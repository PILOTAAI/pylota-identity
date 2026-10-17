# Drafts, approvals and scheduled send

Binding for implementation. An agent writes an email as a **draft** that is stored in its identity's
mailbox, edited with optimistic concurrency, approved by a person or by the operator's own system when
policy or the agent's permissions require it, and sent at once or at a scheduled time. A draft becomes an
outbound message only through the existing send pipeline ([Outbound and safe retries](outbound.md)): the
same policy steps, `TenantQuota` holds, thread lock, idempotency and `uncertain` handling, with the
idempotency key `draft:{draft_id}:{revision}`. An approval binds to the exact content approved, so an edit
after approval asks again. Drafts were a v1.0 non-goal until 2026-10-17
([ADR 0017](../adr/0017-agentmail-parity.md)): Pylota's approval gate needs a server-side place where an
agent's mail waits for an operator, and AgentMail, which Pylota is leaving, has drafts with scheduled send.

| | |
|---|---|
| Requirements | FR-DRF-1 … FR-DRF-14 (this page); changes FR-OUT-3, FR-OUT-13, FR-CON-5, FR-CON-12, FR-CON-15, FR-PRV-3 |
| Edge cases | [Q1](../edge-cases.md)–[Q64](../edge-cases.md) |
| Code | `crates/core/src/drafts.rs` (fields per kind, the state table, the approval requirement, the digest, the etag, `send_at` bounds, the patch merge; pure), `crates/api-types/src/drafts.rs`, `crates/worker/src/handlers/drafts.rs` (the nine routes), `crates/worker/src/mailbox/drafts.rs` (storage, transitions, `SendDraft`, the `drafts` and `draft_index` alarm purposes), `crates/worker/src/mailbox/submit.rs` (the draft arm of the accept transaction), `crates/worker/src/send/{context.rs, authority.rs}` (the send-context loader moved out of `handlers/send.rs`, and the authority check), `crates/worker/src/drafts/attachments.rs` (inline parts and upload references), `crates/worker/src/crons/drafts_due.rs` (the watchdog), `crates/worker/src/auth/people.rs` (the `ApproveDraft` decision), `crates/worker/src/console/pages/drafts.rs`, `crates/worker/src/mcp/tools.rs` (six tools), `crates/cli/src/commands/drafts.rs`, the `drafts` steps of `jobs/retention.rs` and `jobs/erasure.rs` |
| Tables | Mailbox `drafts`, `draft_sync`; new columns `messages.draft_id` and `idempotency.draft_id`; mailbox `meta` keys `alarm:drafts`, `alarm:draft_index`, `draft_index_backoff`; D1 `draft_index` ([Data model](data-model.md)) |
| Contracts | The routes under `/v1/identities/{identity_id}/drafts` and `GET /v1/tenants/{tenant_id}/drafts`; the permissions `drafts:read`, `drafts:write`, `drafts:approve`; the events `draft.approval_requested`, `draft.approved`, `draft.rejected`, `draft.sent`, `draft.failed`; the policy fields `drafts.require_approval` (guard) and `drafts.key_approval` (platform or own partner), and the identity field `send_policy.require_approval`; the errors `404 draft_not_found`, `409 draft_revision_mismatch`, `409 draft_not_editable`, `409 draft_not_sendable`, `409 draft_not_pending`, `409 source_unavailable`, `403 approval_required`, the uploads errors `404 upload_not_found`, `409 upload_not_ready` and `410 upload_expired` on create and update ([Uploads](uploads.md)), `422 draft_limit_reached`, and `403 permission_denied` with `details.reason` `person_required` or `self_approval`; the Message field `draft_id` |
| Limits | [§14](#14-billing-and-limits) (to be copied into [Limits › Drafts and scheduled send](../../reference/limits.md)) |
| Built in | [M29](#23-milestone) |

## 1. Why the mailbox, and a content-free index in D1

**Drafts live in the identity's `IdentityMailbox`.** A draft holds mail content: recipients, subject,
bodies, attachments. Every other piece of mail content lives in the mailbox, where erasure, legal holds,
retention and the per-identity transaction already reach it, and where `submit` runs. Keeping the draft
there means the step that turns a draft into a message is one SQLite transaction: the message row, its
deliveries, the idempotency row and the draft's move to `sent` commit together or not at all
([§5.3](#53-exactly-once)). A D1 draft would need a cross-store protocol for that step, and would put mail
content into the control plane, which today holds none
([Architecture §2](../architecture.md#2-storage)).

**The tenant-wide approval queue is a D1 index.** The console's Overview reads the queue on every render,
and a partner such as Pylota reads it for every operator. A fan-out over every identity's mailbox would
wake idle mailboxes and cost one Durable Object call per identity per read; tenant search already caps such
a fan-out at 100 identities with a 900 ms deadline ([Search](search.md)). So each mailbox mirrors a small
row per draft that is `pending_approval`, `scheduled` or `failed` into D1 `draft_index`
([§9.2](#92-d1-draft_index)): IDs, status, kind, counts, times and key IDs, **never content** (no subject,
address, body, filename or note). The service sign-up ledger keeps its decisions in D1 for the same reason
([Service sign-up ledger §1](service-accounts.md#1-why-d1-and-not-the-mailbox)); drafts differ because the
thing decided is mail content, so the content stays in the mailbox and only the queue's index is in D1.

The mailbox is authoritative. The index can lag the mailbox by the sync delay (normally below a second,
at most the 5-minute back-off during a D1 outage, [§9.3](#93-keeping-the-index-in-step)). Every action
(approve, reject, send, update, delete) goes to the mailbox, which decides on its own row, so a stale index
row can only show something that is no longer waiting; it can never make anything send.

## 2. The draft

### 2.1 Fields

| Field | Meaning |
|---|---|
| `id` | `drf_` + ULID |
| `identity_id`, `tenant_id` | From the path and the key's scope, never from the body |
| `client_id` | Optional caller key for an idempotent create: 1–255 printable ASCII characters without `@` (it appears in events, which hold no addresses), unique per identity ([§4.1](#41-create)) |
| `draft_kind` | `new`, `reply`, `reply_all` or `forward`, fixed at creation from `in_reply_to`, `reply_all` and `forward_of` ([§2.2](#22-kinds-and-the-fields-each-takes)) |
| `kind` | The send kind, as on `POST …/messages`: `transactional` (default), `marketing` (new drafts only) or `auto_reply` (reply drafts only) |
| `status` | [§2.3](#23-states-and-who-moves-them) |
| `revision` | Starts at 1 and increases by one on every successful `PATCH` |
| `etag` | The HTTP entity tag of the current revision ([§2.4](#24-revision-etag-and-digest)) |
| `digest` | The content digest of the current revision, 64 hex characters ([§2.4](#24-revision-etag-and-digest)); `null` once the content is cleared |
| `in_reply_to`, `forward_of` | The source message (`msg_`) of a reply or forward draft |
| `thread_id` | The thread the message will join: the source's thread for replies and forwards, or the `thread_id` given to a new draft; `null` for a new thread |
| `from_address`, `to`, `cc`, `bcc`, `subject`, `text`, `html`, `headers`, `unsubscribe`, `consent`, `include_attachments`, `calendar` | As on the send routes (`calendar` as in [Calendar invitations §5](calendar-invitations.md)), limited per kind ([§2.2](#22-kinds-and-the-fields-each-takes)). `html` is kept exactly as written; it is sanitised only when shown in the console and when the message row is stored |
| `attachments` | `[{ id, filename, content_type, disposition, content_id, size, sha256, source, upload_id }]`: `source` is `inline` (bytes given as base64) or `upload` (copied from an upload when the revision was saved; `upload_id` records which, [§7](#7-attachments)); either way the draft holds its own copy in R2 |
| `labels` | Labels the sent message will carry (`^[a-z0-9][a-z0-9_:-]{0,63}$`, at most 64) |
| `metadata` | As on messages (16 keys, 512 bytes per value); copied to the message |
| `send_at` | The time the draft should be sent, or `null` for "when sent". Setting it alone schedules nothing ([§6](#6-scheduled-send)) |
| `approval` | `{ state, reason, requested_at, requested_by_key_id, approved_revision, approved_at, approved_by_key_id, approved_by_user_id, on_behalf_of, note }`: `state` is `none`, `requested` or `approved`; `reason` says why approval was requested (`requested` by the writer, or the policy level `new_recipients`, `external`, `always`) |
| `send` | The pending or last send request: `{ origin, requested_at, requested_by_key_id, requested_by_user_id, due_at, labels_add, labels_remove }`, `origin` = `api`, `approval` or `console`; `null` when none |
| `rejection` | `{ reason, note, rejected_at, rejected_by_key_id, rejected_by_user_id, on_behalf_of }`, `reason` = `operator` or `expired`; `null` unless rejected |
| `failure` | `{ reason, detail, failed_at, attempts }` while `failed` ([§6.3](#63-the-fire)) |
| `message_id`, `sent_at` | Set when the draft became a message |
| `created_by_key_id`, `updated_by_key_id` | The keys that created the draft and wrote its current revision (`null` for a console write, which never edits content) |
| `created_at`, `updated_at` | Bookkeeping times |

Every text field of a draft was written by an agent that mail may have steered, so the console shows it as
untrusted content ([Console › Showing untrusted mail](console.md#showing-untrusted-mail)), and the
approval screen is the place a person reads it before it leaves.

### 2.2 Kinds and the fields each takes

The kind is fixed at creation. A field a kind does not take is refused with `400 invalid_request`
(`details.errors[].path` names it), at create and on `PATCH`; `in_reply_to`, `reply_all` and `forward_of`
are refused on `PATCH` ([Q4](../edge-cases.md)).

| Request on create | `draft_kind` | Takes | Recipients at send time |
|---|---|---|---|
| neither `in_reply_to` nor `forward_of` | `new` | `to`, `cc`, `bcc`, `subject`, `text`, `html`, `attachments`, `kind` (`transactional` or `marketing`), `thread_id`, `from_address`, `headers`, `unsubscribe`, `consent`, `calendar`, `labels`, `metadata`, `send_at` | From the draft |
| `in_reply_to` | `reply` | `text`, `html`, `attachments`, `kind` (`transactional` or `auto_reply`), `headers`, `calendar`, `labels`, `metadata`, `send_at` | The reply target of the source ([Outbound › Recipients](outbound.md#recipients)), resolved at send time |
| `in_reply_to` with `reply_all: true` | `reply_all` | as `reply`, without `calendar` | The reply target, then the source's `to` and `cc` minus the identity's own addresses, never its `bcc` ([A10](../edge-cases.md)), resolved at send time |
| `forward_of` | `forward` | `to`, `cc`, `bcc`, `text`, `html`, `attachments`, `include_attachments` (default `true`), `from_address`, `headers`, `labels`, `metadata`, `send_at` | From the draft |

`in_reply_to` and `forward_of` together, or `reply_all` without `in_reply_to`, are `400 invalid_request`
([Q2](../edge-cases.md)). The subject of reply and forward drafts is derived when sent, exactly as for the
send routes ([Threading › Outbound threading](threading.md#4-outbound-threading)). The `From` of reply,
reply-all and forward drafts, and of new drafts with `thread_id` and no `from_address`, is chosen by
`select_reply_from` at send time, not at creation (FR-OUT-5,
[Threading §5](threading.md#5-which-address-a-reply-is-sent-from-c3-fr-out-5)): a promotion or a
fallback-pinned thread between creation and sending changes the address, never the approval.

`calendar` follows the calendar rules of the send routes: refused on `reply_all` and `forward` drafts, and
only with `kind: transactional` ([Calendar invitations §5](calendar-invitations.md)); its `ics` text and
`attach_copy` are part of the digest.

A draft may be incomplete while it is edited: a new draft without recipients or subject, a forward without
`to`, a draft without `text` and `html`. Completeness is checked when it is sent or scheduled
([§4.5](#45-send)).

### 2.3 States and who moves them

```text
                 create                       create with request_approval
                   │                                     │
                   ▼     request_approval, or send       ▼
   ┌────────▶ editing ─────── when approval needed ──▶ pending_approval ──── 7 days ──▶ rejected (expired)
   │ PATCH       │                                       │   │   │
   │ (content)   │ send, no approval needed              │   │   └── reject ──────────▶ rejected (operator)
   │             │                                       │   │ approve, send: false
   │             │                                       │   ▼
   │             │                                       │ approved ── send / approve(send) ──┐
   │             │                                       │                                    │
   │             │◀───────── approve(send: true) ────────┘                                    │
   │             ▼                                                                            │
   │       due now? ── no ──▶ scheduled ── alarm at send_at ──┐                               │
   │             │ yes            ▲  │                         │                               │
   │             ▼                │  └── PATCH send_at: null ──┼──▶ approved or editing       │
   │          sending ◀───────────┼────────────────────────────┘◀──────────────────────────────┘
   │             │                │
   │             ├── accepted by submit ──▶ sent (message_id; content cleared)
   │             └── refused ──▶ failed ── PATCH send_at (explicit reschedule) ──┘
   │                     │
   └─────────────────────┴── PATCH content on failed or rejected
   DELETE from any state except sending and sent ──▶ canceled (content cleared)
```

**Actors.** *Writer*: a key with `drafts:write`. *Sender*: a key with `messages:send` and `drafts:read`.
*Approver*: a key with `drafts:approve` that may take the decision ([§4.6](#46-approve)), or a console
owner or admin after re-authentication. *System*: the mailbox alarm, the retention job and erasure.

| # | From | Action | Actor | To | Event |
|---|---|---|---|---|---|
| T1 | – | create | Writer | `editing`, or `pending_approval` with `request_approval: true` | `draft.approval_requested` for the second |
| T2 | `editing`, or `failed` without a valid approval | `PATCH` with `request_approval: true` | Writer | `pending_approval` | `draft.approval_requested` |
| T3 | `editing`, `pending_approval`, `approved`, `scheduled`, `failed` | send with approval needed and no valid approval ([§3](#3-approval-policy)) | Sender | `pending_approval` (unchanged if already there) | `draft.approval_requested`, unless the draft was already `pending_approval` at this revision |
| T4 | `editing`, `pending_approval`, `approved`, `scheduled`, `failed` | send, approval not needed or valid, due now | Sender | `sending`, then `sent`; a refusal answers the error and leaves the draft as it was | `draft.sent` |
| T5 | as T4 | send, approval not needed or valid, `send_at` in the future | Sender | `scheduled` | – |
| T6 | `editing`, `pending_approval`, `failed` | approve, `send: false` | Approver | `approved` | `draft.approved` |
| T7 | `editing`, `pending_approval`, `approved`, `failed` | approve, `send: true`, due now | Approver | `sending`, then `sent` or `failed` | `draft.approved` (not again when already `approved` at this revision), then `draft.sent` or `draft.failed` |
| T8 | as T7 | approve, `send: true`, `send_at` in the future | Approver | `scheduled` | `draft.approved` (as T7) |
| T9 | `editing`, `pending_approval`, `approved`, `scheduled`, `failed` | reject | Approver | `rejected` (`operator`) | `draft.rejected` |
| T10 | `pending_approval` | 7 days after the request, or `send_at` + 1 hour when later ([§13.1](#131-retention)) | System | `rejected` (`expired`) | `draft.rejected` |
| T11 | `scheduled` | the alarm at `send_at` ([§6.3](#63-the-fire)) | System | `sending`, then `sent` or `failed`; `pending_approval` when approval is needed and missing; `scheduled` again (a later attempt) on a transient condition | `draft.sent`, `draft.failed` or `draft.approval_requested` |
| T12 | any but `sending`, `sent`, `canceled` | `PATCH` that changes the digest | Writer | `pending_approval` when the draft was `pending_approval` or held an approval, else `editing` ([§4.3](#43-update)) | `draft.approval_requested` for `pending_approval` |
| T13 | any but `sending`, `sent`, `canceled`, `rejected` | `PATCH` of `send_at`, `labels` or `metadata` only | Writer | unchanged, except `scheduled` with `send_at: null` → `approved` (valid approval) or `editing`, and `failed` with a new `send_at` → `scheduled`, every check running again at the fire ([§6.6](#66-reschedule-unschedule-cancel)) | – |
| T14 | `sending` | the accept transaction | System | `sent` | `draft.sent` |
| T15 | `sending` | a claim older than 2 minutes ([§5.3](#53-exactly-once)) | System | the status before the claim (API origin), or `scheduled` for another attempt (approval or alarm origin; `failed` with `send_interrupted` after the third) | `draft.failed` for the last |
| T16 | any but `sending`, `sent` | `DELETE` | Writer | `canceled` | – |
| T17 | any | erasure, retention purge, identity or tenant deletion | System | row deleted | – |

A `sent` draft is a record: its content is cleared in the transaction that creates the message (the
message holds it now), and the row keeps IDs, the revision, the approval and send metadata and
`message_id` for 30 days ([§13.1](#131-retention)). A `canceled` draft is cleared the same way. `rejected`
keeps its content, so the writer can revise it (T12) or the approver can look back, until retention.

**No send of a rejected revision.** Send and approve on a `rejected` draft answer `409 draft_not_sendable`
or `409 draft_not_pending`; the writer must change the content first (T12), which makes a new revision
([Q63](../edge-cases.md)).

### 2.4 Revision, etag and digest

- **`revision`** increases on every successful `PATCH`, whatever it changes. Approval, scheduling,
  rejection and sending do not change it.
- **`etag`** is `"{revision}-{first 8 hex characters of digest}"`, quoted as HTTP requires (for example
  `"3-9f2c41ab"`), sent in the `ETag` header of every response that returns one draft, and compared with
  `If-Match` by strong comparison. `If-Match: *` matches any existing draft. A cleared draft's etag uses
  `00000000` for the digest part.
- **`digest`** is `core::drafts::digest`, `hex(sha256(canonical_json(input)))` with the canonical JSON of
  [Outbound › Idempotency fingerprint](outbound.md#idempotency-fingerprint) and this input:

  ```json
  { "v": 1, "identity_id": "idn_…", "draft_kind": "new", "kind": "transactional",
    "from_address": null, "thread_id": null, "source_message_id": null,
    "to": [ { "address": "jo@example.net", "name": "Jo Rivera" } ], "cc": [], "bcc": [],
    "subject": "…", "text": "…", "html": null,
    "attachments": [ { "filename": "quote.pdf", "content_type": "application/pdf", "disposition": "attachment",
                       "content_id": null, "size": 48213, "sha256": "…" } ],
    "include_attachments": null, "headers": {}, "unsubscribe": null, "consent": null, "calendar": null }
  ```

  Addresses are normalised (lower case, IDNA A-label domain) and each list is sorted by address, because
  order does not change who receives the mail; display names are kept, because recipients see them. For `reply` and
  `reply_all` drafts, `to` and `cc` are the recipients **resolved** at the moment the digest is computed
  ([§8](#8-replies-and-forwards-the-source-message)), so an approval binds to the people the approver saw.
  `send_at`, `labels` and `metadata` are not in the digest: they do not change what a recipient receives
  or who receives it. A forward's source content is not in the digest; its `source_message_id` and
  `include_attachments` are, and the source is immutable except by erasure ([§8](#8-replies-and-forwards-the-source-message)).

The digest is returned on every draft. An integrator can store it with its own approval record and pass it
back on approve, so the approval names the exact content its person saw ([§4.6](#46-approve)).

## 3. Approval policy

### 3.1 `drafts.require_approval`

A tenant policy field, `never` (default), `new_recipients`, `external` or `always`, applied to **every
send of the tenant's identities** except the system identity's: a draft send, and an ordinary send, reply,
reply-all or forward ([§3.3](#33-ordinary-sends-under-the-policy)). For each recipient of the send (`to`,
`cc` and `bcc`, after the reply recipients are resolved), two facts are computed:

- **Known**: rules 1–3 of FR-OUT-13 ([Outbound › Known recipient](outbound.md#recipient-filters)): the
  identity wrote to it before (`contacts.outbound_count > 0`), a send-allow entry of the tenant names it or
  its `@domain`, or, for a reply or reply-all, it is the reply target of a `received` message with verdict
  `pass`. Rule 4 ([§3.4](#34-approved-recipients-are-known)) is not used here: it is the effect of an
  approval, not a reason to skip one.
- **Internal**: the address is an `active` or `retiring` address of an identity of the **same tenant**
  (D1 `addresses` joined to `identities` on `tenant_id`), or its domain, compared as a lower-case A-label
  and exactly (a subdomain does not match), is the `name` of a domain of the same tenant with
  `ownership_verified_at` set and `state` not `removing` or `removed`. The platform domain never makes an
  address internal by its domain: on it, only the tenant's own identities' addresses are internal, and
  another workspace's address there is external ([D12](../edge-cases.md)). The system identity's address
  is internal to no tenant.

| Level | Approval is needed when |
|---|---|
| `never` | never |
| `new_recipients` | any recipient is neither known nor internal |
| `external` | any recipient is not internal |
| `always` | always |

The levels are ordered `never` < `new_recipients` < `external` < `always`, and each needs approval in
every case the one before it does (a recipient that is neither known nor internal is not internal), so
"stricter" is well defined for the identity override and for the guard rule below. Internal mail needs
approval only at `always`: it does not leave the workspace. `core::drafts::approval_needed(level,
&[RecipientFacts { known, internal }]) -> Option<ApprovalReason>` is the pure function; its answer is the
level that applied, recorded as `approval.reason`.

**Class: guard** ([Workspace policy §2](workspace-policy.md#2-classes-and-ceilings)). A tenant key may set a
stricter value at any time; a looser value only where API keys may take decisions reserved for people
(`key_may_decide`, the existing rule keyed on `quarantine.key_release`); otherwise `403 permission_denied`
with `details.reason = "person_required"` and `details.field` ([Q18](../edge-cases.md)). A console owner or
admin, a platform key and the tenant's partner key may set any value. Pylota Mail Cloud keeps the built-in
default `never` in `PM_DEFAULT_POLICY`: the right level depends on how much the workspace trusts its agents,
and an operator that gives an agent `drafts:write` without `messages:send` gets approval for every send of
that agent whatever the level ([§3.5](#35-the-two-approval-patterns)).

### 3.2 The identity override

`send_policy.require_approval` on an identity (`POST …/identities`, `PATCH /v1/identities/{identity_id}`)
takes the same four values or `null` (inherit, the default). The effective level for a send is the
stricter of the tenant's and the identity's, so an identity can be made stricter than its workspace and
never looser. Writing a looser value than the identity's current one is refused with
`403 scope_denied` (`details.field = "send_policy.require_approval"`) from an identity key, and with
`403 permission_denied` (`person_required`) from a tenant key unless `key_may_decide`; platform and
partner keys may set any value ([Q18](../edge-cases.md)). This is how a workspace with a backend identity
that writes to new people by design (booking confirmations) keeps the tenant at `never` and sets
`new_recipients` on the identity a public chat agent uses.

### 3.3 Ordinary sends under the policy

`POST …/messages`, `…/reply`, `…/reply-all` and `…/forward` gain **policy step 16a**, after the recipient
filters of step 16 have been computed and before the quota step 18 ([Outbound › Policy
pipeline](outbound.md#policy-pipeline)): when the effective level needs approval for the send, it is
refused with `403 approval_required`, `details.reason` = the level, `details.recipients` = the addresses
that made it needed (for `always`, every recipient), and `fix`: "Create a draft with the same content and
send it; it waits for a person's approval." Nothing is stored, no hold is taken and no idempotency record is
written, so the agent can turn the same request into a draft ([Q27](../edge-cases.md)). A dry run returns
the same error. Without this step an agent holding `messages:send` would bypass the policy by not using a
draft. The system identity is exempt, as it is from the abuse pause. At `never` the step does nothing, so
existing tenants see no change. Calendar replies ([Calendar invitations](calendar-invitations.md)) are
sends and pass the same step.

Step 16a reads two more D1 facts in the handler's context batch, only when the effective level is
`new_recipients` or `external`: the directory rows of the recipients with their identity's `tenant_id`
(already read for test tenants, policy step 8) and the tenant's verified domain names. Both are bound as
one JSON array each ([Inbound › Storage caps](inbound.md#storage-caps)).

### 3.4 Approved recipients are known

FR-OUT-13 gains **rule 4**: for a send made from a draft whose current revision carries a **valid
approval** ([§5.2](#52-authority-and-approval-at-send-time)), every recipient of the approved revision is
known for that send. An approval is a person's or the operator system's explicit decision to write to those
addresses, which is what the known-recipient rule exists to require; without rule 4, `new_recipients`
would ask for approval and `require_known_recipient` would then suppress the approved recipient anyway.
Rule 4 never overrides a suppression, a send-block entry or `send_allowlist_only`: those are the tenant's
own refusals and still make the delivery `suppressed` ([Q29](../edge-cases.md)). After the send, the
recipients whose delivery was `queued` have `outbound_count + 1` like any send, so later mail to them is
known by rule 1 without another approval. Rule 4 is narrower than a send-allow entry: it covers one send
from one identity, where an entry is tenant-wide and lasts until deleted.

### 3.5 The two approval patterns

1. **The agent holds `messages:send`; the policy requires approval.** The agent creates a draft and calls
   send. When approval is needed (§3.1), the draft moves to `pending_approval` and the send answers `202`
   with that status (T3). An approver approves with `send: true`, and the draft is sent at once, or at
   `send_at` if that is in the future (T7, T8). If the agent instead sends the content directly, step 16a
   refuses it.
2. **The agent holds `drafts:write` and not `messages:send`.** The agent can never send: it creates the
   draft with `request_approval: true` (or sets it later), and an approver approves with `send: true`,
   which sends with the approver's authority (T7, T8). This needs no policy: it holds whatever
   `drafts.require_approval` says, and it is how Pylota confines every untrusted path
   ([§19](#19-pylota-agentic-os)).

In both patterns the approval is bound to the digest, so an edit after approval asks again (T12), and the
sent message carries `draft_id`, so the decision trail survives the draft record.

## 4. Routes

| Route | Permission | Key levels | `Idempotency-Key` |
|---|---|---|---|
| `POST /v1/identities/{identity_id}/drafts` | `drafts:write` | every level | optional |
| `GET /v1/identities/{identity_id}/drafts` | `drafts:read` | every level | – |
| `GET /v1/identities/{identity_id}/drafts/{draft_id}` | `drafts:read` | every level | – |
| `GET /v1/identities/{identity_id}/drafts/{draft_id}/attachments/{attachment_id}` | `drafts:read` | every level | – |
| `PATCH /v1/identities/{identity_id}/drafts/{draft_id}` | `drafts:write` | every level | – (`If-Match`) |
| `DELETE /v1/identities/{identity_id}/drafts/{draft_id}` | `drafts:write` | every level | – |
| `POST /v1/identities/{identity_id}/drafts/{draft_id}/send` | `messages:send` and `drafts:read` | every level | **required** (optional on a dry run) |
| `POST /v1/identities/{identity_id}/drafts/{draft_id}/approve` | `drafts:approve`, and `messages:send` when `send` is `true` | platform, partner, tenant | optional |
| `POST /v1/identities/{identity_id}/drafts/{draft_id}/reject` | `drafts:approve` | platform, partner, tenant | optional |
| `GET /v1/tenants/{tenant_id}/drafts` | `drafts:read` | platform, partner, tenant | – |

`drafts:write` includes `drafts:read`, and `drafts:approve` includes `drafts:read`, as
`accounts:approve` includes `accounts:request`. Identity keys can never hold `drafts:approve`
(`POST /v1/keys` answers `400 invalid_request`, `details.reason = "permission_not_allowed_for_level"`), so
an agent can never approve its own mail.

**Common rules.** Every route resolves the identity from the path within the key's scope first
([Security § 5.2](security.md#52-order-of-checks)): a draft of another identity, tenant or partner, of the
system identity, or of a `deleting` or `deleted` identity, answers `404 draft_not_found` (or
`404 identity_not_found` for the identity), exactly as a missing one (NFR-SEC-1). A suspended tenant gets
`403 tenant_suspended` on every write (create, update, delete, send, approve, reject); reads still work.
A paused identity may still create, edit, approve (without sending) and reject drafts: drafting stores
mail, as receiving does (FR-IDN-3), and nothing leaves; send, approve with `send: true` and the scheduled
fire refuse or fail with `identity_paused` ([Q57](../edge-cases.md)). Idempotency records of these routes
live in the mailbox's `idempotency` table, scoped per identity like every mail route
([REST API › Idempotency](../../reference/api.md#idempotency)), with the operation (`draft_create`,
`draft_send`, `draft_approve`, `draft_reject`) inside the fingerprint, so a key used for one operation and
then another is `409 idempotency_conflict`. No record holds content: the response is stored as a reference
(`{"$ref": {"kind": "draft", "identity_id": …, "id": …}}`), and a replay reads the draft again with the
calling key's visibility and answers with `"deduplicated": true` and `Idempotent-Replayed: true`. A draft
erased since replays its `404`.

### 4.1 Create

`POST /v1/identities/{identity_id}/drafts`:

```json
{
  "client_id": "pm:v1:draft:apv_8812",
  "to": [ { "address": "jo@example.net", "name": "Jo Rivera" } ],
  "subject": "Your quote for the Golf, 14–16 November",
  "text": "Hi Jo, here is the quote we discussed for the Golf…",
  "attachments": [
    { "upload_id": "upl_01JA2B3C4D5E6F7G8H9J0K1M2N" },
    { "filename": "terms.pdf", "content_type": "application/pdf", "content_base64": "JVBERi0xLjcK…",
      "disposition": "attachment" }
  ],
  "labels": ["quote"],
  "metadata": { "quote_id": "q_118" },
  "send_at": null,
  "request_approval": true
}
```

Rules, in order; the first failure answers and nothing is stored:

1. **Authenticate**, require `drafts:write`, resolve the identity (common rules), charge `RL_API`.
2. **Body**: over 7 MiB → `413 payload_too_large`; JSON or schema errors, a field the kind does not take
   ([§2.2](#22-kinds-and-the-fields-each-takes)), `in_reply_to` with `forward_of`, or `reply_all` without
   `in_reply_to` → `400 invalid_request` with `details.errors[]`.
3. **Field bounds**, with the send routes' validators so a draft never holds what a send would refuse:
   each recipient a valid address (`400 address_invalid`, `400 address_unsupported` for an SMTPUTF8 local
   part); `to` + `cc` + `bcc` at most the tenant's `max_recipients` (`400 too_many_recipients`); subject at
   most 998 characters; `text` at most 524,288 bytes and `html` at most 1,048,576 bytes of UTF-8 (the stored
   message caps of [Inbound › Storage caps](inbound.md#storage-caps), so a draft is never cut when it
   becomes a message); at most 32 attachments, valid base64, a sanitised filename, a valid
   `type/subtype`, `content_id` on inline parts; headers by [Outbound › Headers](outbound.md#headers)
   (`400 header_not_allowed`, `400 invalid_request`); labels and metadata within their limits; `send_at`
   within its window ([§6.1](#61-the-window)); `kind: marketing` with `unsubscribe` and `consent`
   (`400 marketing_requirements_missing`, checked again at send); `calendar` by the calendar step 9a of
   [Calendar invitations §5.2](calendar-invitations.md#52-validation-in-pipeline-order) (`400 calendar_invalid`,
   `413 calendar_too_large`, checked again at send). All `400 invalid_request` unless named.
4. **`MailboxRequest::CreateDraft`** (10-second deadline):
   1. With an `Idempotency-Key`: the idempotency lookup (replay, `409 idempotency_conflict`, or
      `409 request_in_progress` while in flight).
   2. With a `client_id`: an existing draft with it and the same `client_fingerprint` (SHA-256 of the
      canonical create body) answers `200` with that draft and `Idempotent-Replayed: true`; a different
      body answers `409 client_id_conflict` with `details.draft_id` ([Q1](../edge-cases.md)).
   3. The source of a reply or forward draft ([§8](#8-replies-and-forwards-the-source-message)): not in
      this mailbox, `hidden` or `throttled`, or `quarantined` or `screening` without `quarantine:review` →
      `404 message_not_found`.
   4. The identity has 1,000 drafts that are not `sent` or `canceled` → `422 draft_limit_reached` with
      `details.limit` ([Q8](../edge-cases.md)).
   5. Attachments ([§7](#7-attachments)): upload references checked and their bytes copied to the draft
      (`404 upload_not_found`, `409 upload_not_ready`, `410 upload_expired`); inline
      parts written to R2, each under a `meta` marker `draft_att:{att}`; the composed-size estimate checked
      when the tenant's `large_attachments` is `refuse` (`413 message_too_large`).
   6. One transaction: re-check the limit and the `client_id`; insert the draft (`revision = 1`, the
      digest, `created_by_key_id = updated_by_key_id` = the key); with `request_approval: true`, status
      `pending_approval`, `approval = { state: requested, reason: requested, requested_at, requested_by_key_id }`
      and the event `draft.approval_requested`, else `editing`; the `draft_sync` row when the status is
      indexed ([§9.3](#93-keeping-the-index-in-step)); the idempotency row; delete the markers.
5. **Respond** `201` with the [draft](#2-the-draft), its `ETag`, and `"deduplicated": false`. A failure after
   step 4.5 deletes the inline objects it wrote (best effort; the orphan sweep removes any left,
   [§7](#7-attachments)).

### 4.2 List, get and attachment bytes

- `GET /v1/identities/{identity_id}/drafts`: filters `status` (one value or a comma list), `draft_kind`,
  `label`, `thread_id`, `client_id`, `scheduled_after` and `scheduled_before` (on `send_at`, RFC 3339;
  only drafts with a `send_at`), with the standard cursor pagination, newest first (`created_at DESC`,
  `id DESC`). Items are drafts with `text` and `html` set to `null` and a `preview` of at most 240
  characters of `text` (or of text derived from `html`), so a page stays small.
- `GET /v1/identities/{identity_id}/drafts/{draft_id}`: the whole draft, with `ETag`.
- `GET …/drafts/{draft_id}/attachments/{attachment_id}`: the bytes, with the serving headers of
  [Security § 8.5](security.md#85-serving-attachments-and-raw-mime) (`Content-Disposition: attachment`,
  `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox`). A draft attachment whose copy is gone
  answers `404 attachment_not_found`.
- **Visibility.** A reply or forward draft whose source is `quarantined` or `screening` is listed and
  readable only by keys holding `quarantine:review`; any other key gets `404 draft_not_found`, as it would
  for the source itself ([Security § 5.3](security.md#53-cross-level-read-access), [Q3](../edge-cases.md)).

### 4.3 Update

`PATCH /v1/identities/{identity_id}/drafts/{draft_id}`, with an optional `If-Match: "<etag>"`:

```json
{ "text": "Hi Jo, here is the corrected quote…", "attachments_remove": ["att_01JA…"],
  "attachments_add": [ { "upload_id": "upl_01JA…" } ], "labels_add": ["revised"], "send_at": "2026-10-18T08:00:00Z" }
```

- **Semantics.** A field left out is unchanged; `null` clears a nullable field (`subject`, `text`, `html`,
  `from_address`, `thread_id`, `send_at`, `unsubscribe`, `consent`); `to`, `cc`, `bcc`, `headers` and
  `metadata` are replaced whole; attachments change only through `attachments_add` and
  `attachments_remove` (IDs), labels through `labels`, or `labels_add` and `labels_remove`.
  `request_approval: true` asks for approval. An empty patch is `400 invalid_request`.
- **Rules, in order:** authenticate (`drafts:write`), resolve, `403 tenant_suspended`, validate as on
  create (step 2 and 3), then `MailboxRequest::UpdateDraft`:
  1. Load the draft (`404 draft_not_found`, visibility of §4.2). `sending` → `409 draft_not_editable`
     with `details.status` and `retryable: true`; `sent` or `canceled` → `409 draft_not_editable`
     (`retryable: false`) ([Q11](../edge-cases.md)).
  2. `If-Match` given and not the current etag → `409 draft_revision_mismatch` with
     `details.current_revision` and `details.current_etag`; nothing is written ([Q9](../edge-cases.md)).
  3. Merge, check attachments (as create step 4.5), and compute the new digest (reply recipients resolved
     now).
  4. One transaction: re-read the draft; if its revision moved since step 1, answer
     `409 draft_revision_mismatch` when `If-Match` was given, otherwise merge the patch onto the newer
     revision and continue (last writer wins). Then `revision + 1`, `updated_by_key_id`, the new digest,
     and the status rule below; the `draft_sync` row; the events; delete markers. After commit, delete
     the R2 objects of removed inline attachments.
  5. Respond `200` with the draft and its `ETag`.
- **Status rule, when the digest changed (T12).** The approval and the pending send request are dropped
  (`approval.approved_*` cleared, `send = null`). The new status is `pending_approval` when the draft was
  `pending_approval`, held an approval (`approval.state` was `approved`), or the patch has
  `request_approval: true`; otherwise `editing`. A move to, or an edit within, `pending_approval` emits
  `draft.approval_requested` with the new revision, so an approver's screen refreshes and an approval of
  the old revision fails ([Q19](../edge-cases.md)). A `scheduled` draft leaves the schedule; its `send_at`
  is kept as the wanted time, and a new send or approval schedules it again.
- **When only `send_at`, `labels` or `metadata` changed (T13).** The status, the approval and the send
  request stay, with these exceptions: on a `scheduled` draft, a new `send_at` reschedules it
  ([§6.6](#66-reschedule-unschedule-cancel)) and `send_at: null` unschedules it, to `approved` when its
  approval is still valid, else `editing`; on a `failed` draft, a new `send_at` schedules it again (an
  explicit act; the fire re-runs every check, [§6.3](#63-the-fire)).
- **A rejected draft** changes to `editing` (or `pending_approval` with `request_approval: true`) on a
  content change. `request_approval: true` without a content change on a `rejected` draft is
  `409 draft_not_sendable` with `details.status = "rejected"`: the rejected revision cannot be submitted
  again ([Q63](../edge-cases.md)).

### 4.4 Delete

`DELETE /v1/identities/{identity_id}/drafts/{draft_id}` cancels the draft: from any status except
`sending` and `sent` it becomes `canceled` (T16) in one transaction that clears its content (recipients,
subject, bodies, headers, attachments, labels, metadata), its approval and send request, removes it from
the index and re-arms the alarm; the R2 objects of its inline attachments are deleted after commit. A
scheduled draft is therefore never sent once `DELETE` has answered. Answers `200` with the cleared draft;
on a `canceled` draft, `200` again with no change. `sending` → `409 draft_not_editable`
(`retryable: true`); `sent` → `409 draft_not_editable` (the record expires after 30 days, and message
erasure removes it earlier) ([Q62](../edge-cases.md)).

### 4.5 Send

`POST /v1/identities/{identity_id}/drafts/{draft_id}/send` with `Idempotency-Key`. The body is optional:

```json
{ "send_at": null, "revision": 4, "labels_add": ["sent-by-agent"], "labels_remove": ["needs-review"] }
```

`send_at` left out: send at the draft's `send_at` when it is set and in the future, else now. `send_at:
null`: send now, clearing the draft's `send_at`. A time: store it on the draft and schedule. `revision`, when
given, must be the draft's current revision, so a retried send never sends a revision the caller did not
read; the TypeScript SDK always passes it ([SDKs and skills](sdk-and-skills.md)). `labels_add` and
`labels_remove` change the labels the message will carry ([§5.4](#54-labels-metadata-and-draft_id)).

1. **Authenticate**: `messages:send` and `drafts:read`; resolve; charge `RL_SEND` (per identity, as every
   send) and `RL_API`. `Idempotency-Key` missing → `400 idempotency_key_required`; malformed →
   `400 invalid_idempotency_key`. Body errors → `400 invalid_request`; `send_at` outside the window →
   `400 invalid_request` with `details.errors[0].path = "send_at"`.
2. **`MailboxRequest::SendDraft`** (30-second deadline), with the key's hash, the fingerprint
   (`sha256(canonical_json({"operation": "draft_send", "target": draft_id, "body": body}))`), the body and
   the authority `Key { key_id, permissions }`:
   1. **Idempotency.** A record with the same fingerprint → replay. Another fingerprint, operation or draft
      → `409 idempotency_conflict` with `details.original_draft_id`. In flight → `409 request_in_progress`
      ([Q13](../edge-cases.md)).
   2. **State.** `sending` → `409 draft_not_sendable` (`details.status`, `retryable: true`); `sent` →
      `409 draft_not_sendable` with `details.message_id`; `rejected` or `canceled` →
      `409 draft_not_sendable` ([Q14](../edge-cases.md)). A `revision` in the body that is not current →
      `409 draft_revision_mismatch` ([Q10](../edge-cases.md)).
   3. **Completeness.** At least one recipient (after reply recipients are resolved), a subject on a new
      draft, `text` or `html` → otherwise `400 invalid_request` with the path.
   4. **Source and attachments**: [§8](#8-replies-and-forwards-the-source-message) (`409 source_unavailable`)
      and [§7](#7-attachments) (the draft's own attachment copies; nothing is read from an upload at send).
   5. **Context.** `send::context::load` reads the D1 context of [Outbound › Handler](outbound.md#handler)
      step 6 for the resolved recipients (5-second deadline; `503 unavailable` on failure), with the facts
      of §3.3.
   6. **Approval** ([§3](#3-approval-policy), [§5.2](#52-authority-and-approval-at-send-time)). Needed and
      no valid approval → one transaction: `pending_approval` (T3), `approval = { state: requested,
      reason: <level> }`, the send request (`origin: api`, `due_at` = the scheduled time or `null`, the
      labels), the idempotency row, and `draft.approval_requested` unless the draft was already
      `pending_approval` at this revision. Answer `202`.
   7. **Due later** (a future time) → one transaction: `scheduled` (T5), the send request, `send_at`, the
      idempotency row; re-arm the alarm. Answer `202`.
   8. **Due now** → the claim transaction ([§5.3](#53-exactly-once)), then `submit` with the draft
      ([§5.1](#51-senddraft)). Accepted: the accept transaction makes the draft `sent` and writes the
      idempotency row; answer `202` with the draft (`status: "sent"`, `message_id`). Refused: release the
      claim (the draft returns to its status before the claim), write no record, and answer the error a
      direct send would get (`409 identity_paused`, `402 billing_limit`, `429 daily_cap_reached`,
      `409 thread_busy` and the rest) ([Q31](../edge-cases.md)).
3. **Respond** `202` with the draft and `"deduplicated": false`.

**Dry run.** `?dry_run=true` runs steps 1 (the key is optional and never looked up or recorded), 2.2–2.6,
and for a send due now `submit`'s policy steps 1–17 without storing anything, reserving quota or locking the
thread, exactly as a send's dry run ([Outbound › Dry run](outbound.md#policy-pipeline)). It answers `200`:

```json
{ "would_send": false, "outcome": "pending_approval", "approval_reason": "new_recipients", "send_at": null,
  "recipients": [ { "address": "jo@example.net", "field": "to", "status": "queued" } ] }
```

`outcome` is `send`, `schedule` or `pending_approval`; `would_send` is `false` only for
`pending_approval`. For `pending_approval`, `recipients` shows each recipient as it would be after an
approval (rule 4 applied). Otherwise it answers the error a real send would get, plus
`422 all_recipients_suppressed` and `422 recipient_blocked`, which only dry runs return
([Q34](../edge-cases.md)).

### 4.6 Approve

`POST /v1/identities/{identity_id}/drafts/{draft_id}/approve`:

```json
{ "revision": 3, "digest": "9f2c41ab…", "send": true, "note": "Quote checked against the price list.",
  "on_behalf_of": "usr_8812" }
```

| Field | Rule |
|---|---|
| `revision` | Required: the revision the approver read. An `If-Match` header may be sent too; both must name the current revision |
| `digest` | Optional: the digest the approver read; when given it must equal the current digest |
| `send` | Required boolean: `true` sends now, or at `send_at` when that is in the future; `false` approves only |
| `note` | Optional, at most 500 characters, shown to the writer and in the console |
| `on_behalf_of` | Optional, keys only: the person in the caller's own application who decided, for example a Pylota operator user ID. 1–128 printable ASCII characters with no `@`, so no address enters an audit row. Stored in the approval, the audit row and `draft.approved`. A console request cannot set it: the console records the signed-in person |

Rules, in order:

1. **Permission.** `drafts:approve`; with `send: true` also `messages:send`, checked once the body is
   parsed, as a body-dependent permission ([Security § 5.2](security.md#52-order-of-checks) step 2):
   otherwise `403 permission_denied` with `details.required` ([Q21](../edge-cases.md)).
2. **Who.** Approving lets mail leave, so for an API key it is a decision reserved for people
   ([Workspace policy §3](workspace-policy.md#3-decisions-reserved-for-people)), with its own opt-in:
   `auth::people::key_may_decide(Decision::ApproveDraft, …)` is true when `PM_QUARANTINE_KEY_RELEASE` is
   `on` or `PM_CONSOLE` is `off`, or, where the tenant's policy has `drafts.key_approval: true`, when the
   key is a platform key or the partner key of the tenant's own partner (never a tenant key, the rule
   [J16](../edge-cases.md) sets for `quarantine.key_release`); otherwise `403 permission_denied` with
   `details.reason = "person_required"` ([Q16](../edge-cases.md)).
   `quarantine.key_release` does not open this decision: releasing inbound mail and approving outbound
   mail are different risks, and a workspace can open one without the other. A console session is a
   person and is not asked.
3. **Body** (`400 invalid_request`).
4. **`MailboxRequest::ApproveDraft`**, with the approver (`Key { key_id, permissions }` or
   `Person { user_id, role }`):
   1. With an `Idempotency-Key`: the lookup, as on send.
   2. **State.** `editing`, `pending_approval`, `approved` and `failed` may be approved; `scheduled` answers
      `200` with no change (it is approved and scheduled); any other status → `409 draft_not_pending` with
      `details.status` ([Q22](../edge-cases.md)).
   3. **Binding.** `revision` not current, or `digest` given and not equal to the digest computed now (reply
      recipients resolved now) → `409 draft_revision_mismatch` with `details.current_revision` and, for a
      digest, `details.reason = "digest"` ([Q10](../edge-cases.md), [Q25](../edge-cases.md)).
   4. **Not your own.** An API key that wrote the current revision (`updated_by_key_id`) cannot approve it:
      `403 permission_denied` with `details.reason = "self_approval"` ([Q20](../edge-cases.md)). An approval
      checks content someone else wrote.
   5. **Complete.** Completeness, source and uploads as send steps 2.3 and 2.4: an incomplete draft cannot
      be approved, with either value of `send`.
   6. **One transaction.** On a draft already `approved` at this revision, the approval is kept as it is
      and no event is written; otherwise `approval = { state: approved, approved_revision, approved_digest,
      approved_at, approved_by_key_id | approved_by_user_id, on_behalf_of, note }` and `draft.approved`
      (with `send`). Then: `send: false` → `approved` (T6), any earlier send request dropped; `send: true`
      and due later → `scheduled` with the send request (`origin: approval`, or `console`, and the
      approver as its authority; the labels of an earlier send request are kept) (T8); `send: true` and
      due now → the claim (T7). The idempotency row, unless sending now.
   7. **Sending now**: `submit` with the approver as authority ([§5.1](#51-senddraft)). Accepted → `sent`
      (`draft.sent`). Refused → one transaction: `failed` with `failure = { reason: <the error code>,
      detail, failed_at, attempts: 1 }`, the claim released, `draft.failed`. The approval stays: the content
      was approved, and a later explicit send needs no second approval. A transient condition
      ([§6.4](#64-transient-conditions-and-the-late-limit)), such as D1 unavailable when the send context is
      loaded, is not a refusal: the draft becomes `scheduled` with `send_at` = now and its next attempt 30
      seconds later, so the approver's decision is carried out by the alarm. Then the idempotency row.
5. **Respond** `200` with the draft. A refused send is not an error of the approval: the response shows
   `status: "failed"` and `failure`.

An approval after `send_at` has passed sends at once: a person decided with the time in front of them (the
console says "The scheduled time has passed; approving sends it now", [§16](#16-console)). An approval of a
`pending_approval` draft that has expired fails: expiry has already made it `rejected`
([Q23](../edge-cases.md), [Q41](../edge-cases.md)).

**Audit.** `draft.approve` with `actor_key_id` or `actor_user_id`, and `details_json = { draft_id,
revision, digest (first 16 hex characters), send, on_behalf_of, recipient_count, via }`. The mailbox cannot
write D1 inside its transaction, so the audit row travels with the event: when the outbox dispatch writes
`draft.approved` to `event_index`, the same D1 batch inserts the audit row with `INSERT OR IGNORE` and a
deterministic `aud_` ID (a ULID whose time is the event's `occurred_at` and whose random part is the first
10 bytes of `HMAC-SHA256(PM_HASH_KEY, "audit:" + event_id)`), so a repeated dispatch writes it once and a
decision is never committed without its audit row eventually following
([Webhooks › Dispatching](webhooks.md#dispatching-alarm-purpose-outbox)). `draft.reject` works the same way.

### 4.7 Reject

`POST /v1/identities/{identity_id}/drafts/{draft_id}/reject` with
`{ "reason": "Too pushy; offer the weekend rate instead.", "revision": 3, "on_behalf_of": "usr_8812" }`.
`reason` is required, 1–500 characters (the writer needs it to revise); `revision` is optional and, when
given, must be current (`409 draft_revision_mismatch`); `on_behalf_of` as for approve. Rejecting can only
keep mail from leaving, so it is not a decision reserved for people: any key with `drafts:approve` may
reject. `editing`, `pending_approval`, `approved`, `scheduled` and `failed` become `rejected` (T9) in one
transaction that drops the approval and send request, removes the draft from the schedule and the index,
and emits `draft.rejected` with `reason: "operator"`; audit `draft.reject`. `rejected` answers `200` with no
change; `sending` → `409 draft_not_editable` (`retryable: true`); `sent` and `canceled` →
`409 draft_not_pending`.

### 4.8 The tenant's queue

`GET /v1/tenants/{tenant_id}/drafts` reads D1 `draft_index`, never a mailbox:

- **Filters**: `status` (`pending_approval`, the default, `scheduled` or `failed`, or a comma list),
  `identity_id` (outside the key's scope → `404 identity_not_found`), `scheduled_after`,
  `scheduled_before`. An identity key gets `403 scope_denied` ([F3](../edge-cases.md) rule).
- **Order**: oldest first (`status_at ASC`, `draft_id ASC`), because it is a queue. The cursor holds that
  keyset in the standard cursor format ([REST API › Pagination](../../reference/api.md#pagination)).
- **Items** (no content):

  ```json
  { "draft_id": "drf_01JA…", "identity_id": "idn_01J9…", "status": "pending_approval", "draft_kind": "new",
    "revision": 3, "approval_reason": "new_recipients", "recipient_count": 1, "attachment_count": 2,
    "send_at": null, "status_at": "2026-10-17T09:12:00Z", "failure_reason": null,
    "requested_by_key_id": "key_01J9…", "updated_at": "2026-10-17T09:12:00Z" }
  ```

To read a draft's content, get it by ID on its identity.

### 4.9 Errors by route

| Code | Routes | When |
|---|---|---|
| `400 invalid_request` | all writes | Schema, a field the kind does not take, bounds, `send_at` window, an incomplete draft on send or approve, a missing `revision` or `send` on approve, a bad `on_behalf_of` |
| `400 idempotency_key_required`, `invalid_idempotency_key` | send | As for sends |
| `400 address_invalid`, `address_unsupported`, `too_many_recipients`, `header_not_allowed`, `marketing_requirements_missing`, `calendar_invalid`; `413 calendar_too_large` | create, update | As for sends |
| `403 permission_denied` | all | A missing permission (`details.required`); approve by a key where people decide (`person_required`); approve by the key that wrote the revision (`self_approval`) |
| `403 scope_denied` | tenant list; identity `send_policy.require_approval` | An identity key on the tenant list; an identity key loosening its own override |
| `403 tenant_suspended` | all writes | The tenant is suspended |
| `403 approval_required` | `POST …/messages`, `…/reply`, `…/reply-all`, `…/forward` | Step 16a ([§3.3](#33-ordinary-sends-under-the-policy)) |
| `404 draft_not_found`, `identity_not_found`, `message_not_found`, `attachment_not_found` | as named | Missing or out of scope, indistinguishably |
| `409 client_id_conflict` | create | Same `client_id`, another body |
| `409 draft_revision_mismatch` | update, send, approve, reject | Stale `If-Match`, `revision` or `digest` |
| `409 draft_not_editable` | update, delete, reject | `sending` (`retryable: true`), `sent`, `canceled` |
| `409 draft_not_sendable` | send, update (`request_approval`) | `sending` (`retryable: true`), `sent` (`details.message_id`), `rejected`, `canceled` |
| `409 draft_not_pending` | approve, reject | A status that cannot be decided (`details.status`) |
| `409 source_unavailable` | send, approve | [§8](#8-replies-and-forwards-the-source-message) (`details.reason`) |
| `404 upload_not_found`, `409 upload_not_ready`, `410 upload_expired` | create, update | [§7](#7-attachments), with the codes and `details` of [Uploads](uploads.md) |
| `409 idempotency_conflict`, `request_in_progress` | create, send, approve, reject | As for sends |
| `409` and `402`, `429` codes of the send pipeline | send, approve with `send: true` | Answered by send; recorded as `failure.reason` by approve |
| `410 cursor_expired` | lists | As for every list |
| `413 payload_too_large`, `message_too_large` | create, update | Body over 7 MiB; own parts over the composed limit with `large_attachments: refuse` |
| `422 draft_limit_reached` | create | 1,000 drafts that are not `sent` or `canceled` |
| `422 all_recipients_suppressed`, `recipient_blocked` | send dry run | As for a send's dry run |
| `503 unavailable` | all | D1 or the mailbox is unavailable, or the deployment is frozen or read-only |

## 5. Sending a draft: the one pipeline

### 5.1 `SendDraft`

Every send of a draft, whether from `POST …/send`, from an approval with `send: true` or from the alarm,
runs `mailbox::drafts::send_draft`, inside the identity's mailbox, which calls the existing `submit`
([Outbound › The submit path](outbound.md#the-submit-path)) with:

```rust
// crates/worker/src/mailbox/submit.rs: SubmitInput gains one optional field and one flag
pub struct DraftRef { pub draft_id: String, pub revision: u32, pub claim_token: String }
pub struct SubmitInput {
    // … the existing fields: operation, target, body, idempotency key hash, fingerprint, key scope …
    pub draft: Option<DraftRef>,     // Some for a draft send; the accept transaction finishes the draft
    pub approved: bool,              // the draft's current revision has a valid approval: FR-OUT-13 rule 4
}
```

- **Operation and target.** `new` → `send` (with `thread_id` when set), `reply` → `reply`, `reply_all` →
  `reply_all`, `forward` → `forward`, the target being the identity or `{identity_id}/{source_message_id}`,
  as for the REST routes.
- **Body.** Built from the draft's stored fields in the request shape of the matching send route, with each
  attachment's bytes read from R2 ([§7](#7-attachments)) and the message's labels
  ([§5.4](#54-labels-metadata-and-draft_id)).
- **Idempotency key** `draft:{draft_id}:{revision}`; its SHA-256 is the ledger's `key_hash` and the sent
  copy's `idem_key_sha256` R2 metadata, as for any send, so a restore rebuilds it
  ([Outbound › Composition](outbound.md#composition)). The fingerprint is
  `send_fingerprint(operation, target, body)` over the draft's fields at that revision, without labels
  and metadata, so a label change between two attempts of one revision never conflicts.
- **D1 context.** The draft path has no request handler in front of the mailbox for the alarm, and the
  handler of `POST …/send` cannot know a reply draft's recipients, so the mailbox loads the context
  itself with `send::context::load(d1, tenant_id, identity_id, &recipients)`, the function
  `handlers/send.rs` uses (moved to `crates/worker/src/send/context.rs` by M29), with the same 5-second
  deadline.
- **Policy.** Every step of [Outbound › Policy pipeline](outbound.md#policy-pipeline) runs, step 16a
  included, with the authority's permissions where a step reads the key (step 5's visibility of a
  quarantined source). There is no draft shortcut: suppression, lists, caps, holds, the lock, size,
  marketing and loop rules apply exactly. If step 16a refuses a draft send (a recipient fact changed
  between the draft's own approval check and `submit`), the draft goes to `pending_approval` as in
  [§5.2](#52-authority-and-approval-at-send-time) step 1, never to `failed`.

### 5.2 Authority and approval at send time

**Authority** is who sends: `Key { key_id }` or `Person { user_id }`.

| Origin | Authority | Checked |
|---|---|---|
| `POST …/send` | The calling key | By authentication, now |
| Approve with `send: true` | The approver | By the approve route, now |
| The alarm (a scheduled send) | The authority recorded in the send request when it was scheduled | Re-read from D1 at the fire (`send::authority::reload`): a key must be neither revoked nor expired, still hold `messages:send` (and `quarantine:review` when the source is `quarantined` or `screening`), still reach the identity, and its partner must not be suspended; a person must be an `active` user with a `members` row for the tenant whose role holds `messages:send` (owner or admin) |

**A valid approval** at send time means: `approval.state` is `approved`, `approval.approved_digest`
equals the digest computed **now** (reply recipients resolved now), and the approver is still acceptable,
re-read from D1 like an authority: a key neither revoked nor expired, still holding `drafts:approve`, with
`key_may_decide(Decision::ApproveDraft, …)` still true for its tenant; or a person who is still an active
owner or admin of the workspace.

The checks run in this order, so the safest outcome wins:

1. Approval needed (§3.1 with the current facts) and no valid approval → the draft goes back to
   `pending_approval` with `draft.approval_requested` (`approval.reason` = the level). It is not `failed`:
   the content may be fine, and a person is asked again ([Q25](../edge-cases.md), [Q26](../edge-cases.md)).
   A scheduled draft keeps its `send_at`; when it is approved later with `send: true`, it is scheduled
   again, or sent at once if that time has passed.
2. Authority not valid → `failed` with `authority_revoked` ([Q26](../edge-cases.md)).
3. Otherwise `submit` runs with `approved` = whether a valid approval exists (rule 4 applies only then).

### 5.3 Exactly once

A draft revision produces at most one message, whatever crashes, retries, concurrent requests or repeated
alarms occur ([Q12](../edge-cases.md), [Q32](../edge-cases.md), [Q36](../edge-cases.md)):

1. **The claim.** Before any `.await` of the send, one transaction moves the draft to `sending` and writes
   `claim = { token, claimed_at, prior_status, origin, attempt }` (the `drafts.claim_json` column), only
   if the draft is still in a status that may be sent and has no claim younger than 2 minutes. A second
   request, the alarm or an approval that finds `sending` stops there (`409 draft_not_sendable` with
   `retryable: true` for an API caller; the alarm skips it). The Durable Object runs one request at a time
   between `.await`s, and alarms may run concurrently with requests
   ([Durable Objects › State › abort](https://developers.cloudflare.com/durable-objects/api/state/), read
   2026-10-17), so the claim, not the caller, decides.
2. **The in-flight set.** `submit`'s in-memory `in_flight` set holds the draft's internal key while the
   send runs, as for any send ([Outbound › Reservation](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1)).
3. **The accept transaction.** `submit`'s step 7 transaction (message, deliveries, idempotency row, lock)
   gains the draft arm: it re-reads the draft, checks that it is `sending` with this `claim.token` and this
   `revision`, and in the same transaction sets `status = 'sent'`, `message_id`, `sent_at`, clears the
   content and the claim, deletes the inline attachment objects' keys from the row (the objects are
   deleted after commit), writes the `draft_sync` delete row, writes the idempotency row of the
   `POST …/send` or approve request when there is one, and appends `draft.sent`. If the check fails (the
   draft was canceled, rejected or erased while the send was composing), the transaction is abandoned and
   `submit` takes its existing refusal path (the sent copy and the marker deleted, the reservations
   released, [I9](../edge-cases.md)), so no message exists. **A message exists if and only if the draft
   is `sent`.**
4. **The internal key.** Because a refused send records nothing, a later attempt of the same revision
   (an explicit resend after a `402`, for example) reuses `draft:{draft_id}:{revision}` and is evaluated
   again, as FR-BILL-6 wants ([Q49](../edge-cases.md)). If a bug ever let a second attempt of a sent
   revision reach `submit`, the idempotency row would replay the first message and the draft arm would
   find the draft already `sent` with that `message_id` and change nothing.
5. **A claim that dies.** A claim older than 2 minutes means the attempt died before the accept
   transaction (the `submit` deadline is 30 seconds), so by rule 3 no message exists. The mailbox's
   `drafts` alarm purpose resolves it (T15): for `origin: api`, the draft returns to `prior_status` and the
   caller, who got no answer, retries with the same `Idempotency-Key`; for `origin: approval`, `console` or
   `alarm`, the draft becomes `scheduled` with `send_at` = now and `claim.attempt + 1`, so the alarm tries
   again; the third interrupted attempt makes it `failed` with `send_interrupted` and `draft.failed`.
   `meta` `alarm:drafts` covers `claimed_at + 2 minutes` of every claim.

A message accepted by `submit` follows the normal outbound life: transport, `uncertain`, reconciliation,
bounces. The draft is `sent` from the accept transaction on and never sends again; the message's own
events say what happened to it ([Q33](../edge-cases.md)).

### 5.4 Labels, metadata and `draft_id`

- The message's labels are the draft's `labels`, plus the send request's `labels_add`, minus its
  `labels_remove`, at most 64 (more → `400 invalid_request` at the send request). AgentMail carries draft
  labels to the sent message the same way.
- The draft's `metadata` is copied to the message's `metadata`.
- The message row gets `draft_id` (new column `messages.draft_id`), returned as `draft_id` on the
  [Message object](../../reference/api.md#message-object) and in the message summary of events, so the
  decision trail survives after the draft record is purged.

## 6. Scheduled send

### 6.1 The window

`send_at` is an RFC 3339 time, stored as UTC milliseconds. When a draft is scheduled (send, approve with
`send: true`, or a `PATCH` of `send_at` on a `scheduled` or `failed` draft), it must be **at least 60
seconds and at most 90 days** after the request; a `PATCH` that only stores a wanted time on an unscheduled
draft is checked the same way. Outside the window: `400 invalid_request`, `details.errors[0].path =
"send_at"`, `details.reason` `too_soon` or `too_far` ([Q7](../edge-cases.md)). Where the identity has
`retention.message_hours`, `send_at` must also fall before the draft's own expiry under that setting,
otherwise `400 invalid_request` with `details.reason = "beyond_retention"`
([Parity additions § 7.10](parity-additions.md#710-interactions), [Z61](../edge-cases.md)). Both bounds are
Pylota Mail choices:

- **60 seconds.** Anything sooner is a send. A minute gives the writer and the approver a window in which
  `DELETE` or reject reliably wins, and keeps every scheduled send well above the alarm's retry interval
  (2 seconds, below) and the watchdog's 2-minute threshold ([§6.5](#65-catch-up-after-a-missed-alarm)).
- **90 days.** It equals the default raw-MIME retention (`retention.raw_days`), so a forward scheduled
  within the window finds its source as it is today unless the tenant shortened retention; it bounds how
  long a draft keeps its content and attachment copies; and longer schedules (yearly reminders) belong to the
  integrator's own scheduler, which can create the draft nearer the time. AgentMail documents no bound.

### 6.2 One alarm for many drafts

A Durable Object "is able to schedule a single alarm at a time"; a new `setAlarm()` "will override the
existing alarm"; the handler has "at-least-once execution" and is retried when it throws, "starting at 2
second delays for up to 6 retries", after which it runs again only when `setAlarm()` is called; only one
`alarm()` runs at a time per object, and an unexpected termination may re-run it from the start on another
machine ([Alarms](https://developers.cloudflare.com/durable-objects/api/alarms/), last updated 2026-04-21,
read 2026-10-17). Alarms can run concurrently with other requests to the same object
([State › abort](https://developers.cloudflare.com/durable-objects/api/state/), read 2026-10-17). An alarm
handler has at most 15 minutes of wall time
([Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17). The page
does not say what a time in the past does, or how late an alarm may fire.

The mailbox already multiplexes purposes onto its one alarm ([Design conventions §4, rule 5](index.md#4-durable-object-transactions)).
Drafts add two purposes:

- **`alarm:drafts`** = the earliest of: `send_at` of every `scheduled` draft; `next_attempt_at` of a
  scheduled draft waiting after a transient condition ([§6.4](#64-transient-conditions-and-the-late-limit));
  `claimed_at + 2 minutes` of every claim; and the next pending-approval expiry
  ([§13.1](#131-retention)). It is recomputed in every transaction that changes one of them, from the
  index `drafts_due` (`SELECT MIN(…)`), so rescheduling earlier re-arms earlier and cancelling the earliest
  re-arms to the next ([Q35](../edge-cases.md)).
- **`alarm:draft_index`** = now, while a `draft_sync` row waits ([§9.3](#93-keeping-the-index-in-step)).

The object sets its alarm to `max(earliest pending wake-up, now)` after each transaction and on every
wake (any request or alarm), when that is earlier than the current alarm or no alarm is set, so it never
relies on what a past time does and a lost alarm is re-armed by the next request
([Q37](../edge-cases.md)).

**The `drafts` purpose**, when due:

1. On an erased mailbox (`meta.erased = '1'`) it does nothing and arms nothing ([Q42](../edge-cases.md)).
2. It resolves expired claims (§5.3 rule 5) and expired pending approvals (T10).
3. It selects due drafts: `SELECT id FROM drafts WHERE status = 'scheduled' AND COALESCE(next_attempt_at,
   send_at) <= ?now ORDER BY COALESCE(next_attempt_at, send_at), id LIMIT 10`, and fires them one after
   another ([§6.3](#63-the-fire)). Ten fires, each with a 30-second `submit` deadline, stay well inside
   the 15-minute wall time. More due → `alarm:drafts = now`.
4. It never throws. Each fire's errors are caught and recorded on the draft; the handler re-arms before it
   returns, because a handler that keeps throwing stops being retried after six retries.

Fires of one mailbox run one after another, so drafts sent at the same instant take their thread locks in
order. Alarm-driven sends are not charged to `RL_SEND`, which protects the API from request bursts; their
volume is bounded by the daily caps, the allowances and the ten-per-run batch.

### 6.3 The fire

For each due draft, in this order (T11):

1. **Still due?** The draft is `scheduled` and due, re-read in the claim transaction; otherwise skip (a
   cancel, reject or reschedule won).
2. **Late limit.** Now later than `send_at` + 1 hour → `failed` with `send_window_missed`, never sent by
   itself ([Q38](../edge-cases.md)).
3. **Claim** (§5.3, `origin: alarm`).
4. **Source and recipients** ([§8](#8-replies-and-forwards-the-source-message)) → `failed`
   (`source_unavailable`).
5. **Context** (`send::context::load`), and the deployment's switches as the outbound consumer reads them
   (freeze, `read_only`, `free_sending` for a Free or ramped workspace,
   [Observability § 5.6](observability.md#56-automatic-containment)) → transient (§6.4).
6. **Approval, then authority** ([§5.2](#52-authority-and-approval-at-send-time)) → `pending_approval`, or
   `failed` (`authority_revoked`).
7. **Attachment copies** ([§7](#7-attachments)): a stored copy missing or not matching its SHA-256 → `failed`
   (`source_unavailable`).
8. **`submit`** → `sent`, or the outcome below.

Every outcome is defined ([Q39](../edge-cases.md)):

| Condition at the fire | Outcome | `failure.reason` |
|---|---|---|
| The submit accepted it | `sent`, `draft.sent`; delivery continues as for any message | – |
| Every recipient suppressed (a suppression, send-block or allow-list rule) | `sent`: the message is stored `suppressed` and `message.suppressed` is emitted, as for any send (FR-OUT-4); `draft.sent` carries `message_status: "suppressed"` | – |
| Some recipients suppressed | `sent`; those deliveries are `suppressed` | – |
| Approval now needed and missing or no longer valid | `pending_approval`, `draft.approval_requested` | – |
| Identity paused | `failed`; not sent when the identity is resumed (AgentMail behaves the same) | `identity_paused` |
| Tenant suspended | `failed` | `tenant_suspended` |
| The tenant's partner suspended | `failed` | `partner_suspended` |
| Tenant or sending domain sending-paused | `failed` | `sending_paused` |
| No accountable human | `failed` | `identity_owner_required` |
| The `sends` allowance spent | `failed`; `billing.limit_reached` once per period, and the usage alerts already sent at 80% and 100% ([§14](#14-billing-and-limits)) | `billing_limit` |
| A daily cap reached (identity, tenant, shared domain, billing dispute) | `failed`; `failure.detail` has `details.cap` and `resets_at` | `daily_cap_reached` |
| The authority can no longer send | `failed` | `authority_revoked` |
| An explicit `from_address` that is no longer `active` (or `retiring` on its thread), or no longer the identity's | `failed` | `sender_address_unavailable` |
| The `From` chosen by `select_reply_from` (no explicit address) | Used as chosen at the fire; a retired reply address falls to the primary (C3) | – |
| Sending domain `pending` or `verifying` | `failed` | `domain_not_ready` |
| Sending domain `failing` or `suspended` | Accepted; fallback to the platform address at transport time (FR-DOM-6), or `domain_failing_no_fallback` on the message | – |
| `kind: marketing` from a `cloudflare`-transport domain | `failed` | `transport_unavailable` |
| A test tenant's recipient outside the simulator and its own (or its partner's) identities | `failed` | `test_mode_recipient` |
| The hop count reaches 10 | `failed` | `loop_detected` |
| An auto-reply no longer allowed | `failed` | `auto_reply_not_allowed` |
| The composed message over 5 MiB with `large_attachments: refuse` | `failed` | `message_too_large` |
| A new draft's `thread_id` names a thread that no longer exists | `failed` | `thread_not_found` |
| Source erased, not visible to the authority, truncated body with raw MIME gone, or an attachment object missing | `failed` | `source_unavailable` |
| Three interrupted attempts | `failed` | `send_interrupted` |
| Not attempted by `send_at` + 1 hour | `failed` | `send_window_missed` |

`failure.detail` holds the refusal's `details` (never content), and `failure.attempts` the attempts made.
Each `failed` emits `draft.failed` with the reason and puts the draft in the tenant's queue and the
console's "Needs a person" ([§16](#16-console)). **A failed draft is never sent again by itself**: a person
or the writer reschedules it (`PATCH send_at`), sends it (`POST …/send`) or approves it again with
`send: true`, each an explicit act.

### 6.4 Transient conditions and the late limit

Some refusals mean "not now", not "no": `503 unavailable` (D1 or a dependency down, a frozen or read-only
deployment, D1 full), `504 timeout`, `409 thread_busy`, `409 request_in_progress`, the `free_sending`
switch, and a send-context load that failed. For these the fire releases the claim, keeps the draft
`scheduled` and sets `next_attempt_at` = now + 30 s, 1 min, 2 min, 5 min, then every 5 minutes (±10%
jitter), counted in `failure.attempts` without failing it. The **late limit** ends it: a draft not
attempted successfully by `send_at` + 1 hour becomes `failed` with `send_window_missed` and the last
transient code in `failure.detail` ([Q38](../edge-cases.md)).

The late limit is a Pylota Mail choice. Scheduled mail is usually tied to a time ("your pick-up is at
10:00"), and an hour-late send is the most an unattended schedule should risk; a person can still send it
explicitly. The same limit covers a fire delayed by an outage, a restore freeze or a missed alarm. It does
not apply to an approval: a person approving after `send_at` decides with the time shown.

### 6.5 Catch-up after a missed alarm

Three mechanisms make a scheduled draft fire even when an alarm is lost (its retries exhausted, a bug, an
object moved mid-run):

1. **Re-arm on wake** (§6.2): any request to the mailbox re-arms a missing or late alarm.
2. **The watchdog** (`crons/drafts_due.rs`, in the every-minute cron): reads D1, which a stuck mailbox
   cannot influence:

   ```sql
   SELECT DISTINCT identity_id, tenant_id FROM draft_index
   WHERE status = 'scheduled' AND send_at < ?1          -- ?1 = now − 120 000 (2 minutes)
   ORDER BY send_at LIMIT 100;
   ```

   and sends each mailbox `MailboxRequest::DraftsDue`, which runs the `drafts` purpose at once and re-arms.
   Each such poke counts `drafts_alarm_missed_total`; a mailbox that answers that it has nothing due
   (the index lagged) is harmless. Tenants that are `erasing` or `erased`, and suspended tenants, are
   skipped (their drafts fail or are erased by their own paths).
3. **The alert** `drafts_overdue` ([§18](#18-observability)) when any scheduled draft is more than 10
   minutes past `send_at`.

A draft found by catch-up still obeys the late limit, so a missed alarm can delay a send by at most an hour
and never sends it later than that.

### 6.6 Reschedule, unschedule, cancel

| Wanted | How | Result |
|---|---|---|
| Schedule | `POST …/send` with a future `send_at`, or with the draft's own future `send_at`; or approve with `send: true` | `scheduled` |
| Reschedule | `PATCH` `send_at` on a `scheduled` draft | Still `scheduled`, at the new time; the approval and the send request stay, because `send_at` is not in the digest |
| Unschedule | `PATCH` `send_at: null` on a `scheduled` draft | `approved` (valid approval) or `editing`; the send request is dropped |
| Retry a failed scheduled send | `PATCH` `send_at` on a `failed` draft, `POST …/send`, or approve with `send: true` | `scheduled` or sent; every check runs again |
| Send now | `POST …/send` with `send_at: null` | Sent now (or `pending_approval`) |
| Cancel | `DELETE` | `canceled`; the alarm is re-armed without it |
| Stop it as an approver | reject | `rejected` |

A `PATCH` that changes the content of a `scheduled` draft takes it off the schedule (T12): a send request,
like an approval, covers the content it was made for. This also stops a key that holds only
`drafts:write` from changing what another key's `messages:send` will send ([Q40](../edge-cases.md)).

## 7. Attachments

A draft attachment is one of:

- **Inline**: `{ filename, content_type, content_base64, disposition, content_id }`, counted against the
  7 MiB request body. The handler decodes and checks it as a send does
  ([Outbound › Attachments and size](outbound.md#attachments-and-size)); the mailbox computes its SHA-256
  and writes the bytes to R2 at `t/{tenant_id}/i/{identity_id}/drafts/{draft_id}/a/{attachment_id}` before
  the transaction, under a `meta` marker `draft_att:{attachment_id}` that the transaction deletes. The
  rolling orphan sweep ([Inbound › Orphan objects](inbound.md#orphan-objects), [I9](../edge-cases.md))
  also lists `drafts/` keys and deletes an object whose attachment ID is in no draft row and has no marker
  younger than one hour.
- **An upload reference**: `{ upload_id }`, optionally with `filename`, `disposition` and `content_id` to
  override the upload's (a new `filename` is classified again, as [Uploads § 3.5](uploads.md#35-attaching-an-upload)
  requires). The upload must belong to the same identity and be `ready` and unexpired; otherwise the
  uploads page's errors: `404 upload_not_found` (also for another identity's or tenant's upload, so
  nothing is revealed), `409 upload_not_ready` or `410 upload_expired` ([Q6](../edge-cases.md)). When the
  revision is saved, the upload's bytes are **copied** into the draft's own object at
  `t/{tenant_id}/i/{identity_id}/drafts/{draft_id}/a/{attachment_id}`, the copy's SHA-256 checked against
  the upload's, and the attachment row records `upload_id` for provenance only. The draft never reads the
  upload again, so deleting or expiring the upload never changes a saved draft or its scheduled send
  ([Uploads § 5.4](uploads.md#54-reuse-copy-never-reference), [Q108](../edge-cases.md), [Q61](../edge-cases.md)).

**Scheduling and uploads.** Scheduling does not depend on uploads: every saved revision holds its own
copies.

**At send**, the bytes are read from the draft's own objects, their SHA-256 compared with the stored value
(a missing or changed copy, which only a storage fault can cause, fails the send with `source_unavailable`,
`details.reason = "attachment_missing"`), and handed to composition as base64
parts, which writes the message's own copies under `t/{tenant_id}/i/{identity_id}/out/{message_id}/a/…`
with new `att_` IDs ([Outbound › Attachments and size](outbound.md#attachments-and-size)). The draft's
objects (inline parts and upload copies) are deleted after the accept transaction; an upload's own object
follows the uploads design's lifecycle. Size is checked at write time only when `large_attachments` is `refuse` and the draft's own parts
already exceed the composed limit (`413 message_too_large`), so an agent learns before an approver spends
time; the authoritative check is policy step 17 at send, where `large_attachments: "link"` turns oversized
attachments into signed links as for any send. A forward's re-attached source attachments are added at
send, as the forward route adds them.

## 8. Replies and forwards: the source message

- **At creation**, the source (`in_reply_to` or `forward_of`) must be a message of this mailbox that the
  creating key can see: never `hidden` or `throttled`; `quarantined` or `screening` only with
  `quarantine:review` ([Screening](screening.md)); otherwise `404 message_not_found`, as the reply and
  forward routes answer ([Q3](../edge-cases.md), [Q45](../edge-cases.md)). The draft's own visibility then
  follows its source (§4.2).
- **At send**, the source is read again:
  - erased (an erasure deletes drafts with their source, [§13.2](#132-erasure), so this is a race) →
    `source_unavailable` with `details.reason = "erased"`;
  - `quarantined` or `screening` and the authority lacks `quarantine:review` → `source_unavailable`
    (`not_visible`);
  - reply recipients are resolved now with the reply-target rules ([D3](../edge-cases.md),
    [A10](../edge-cases.md)); a change since the approval changes the digest and asks again
    ([§5.2](#52-authority-and-approval-at-send-time));
  - a forward is built now from the source row and its attachment objects, as the forward route builds it
    (transfer note, original text, sanitised HTML, attachments without `risk` when `include_attachments`):
    it snapshots the source at send time, not at creation. When the source's stored body was cut at its
    storage cap (flag `body_truncated`) and its raw MIME is gone (`retention.raw_days` passed), the
    forwarded copy would silently be incomplete, so the send fails with `source_unavailable`
    (`raw_expired`); when an attachment object it re-attaches is missing, with `attachment_missing`
    ([Q43](../edge-cases.md)). A source whose stored body is complete forwards whether or not its raw MIME
    is still kept;
  - the automatic-reply rules (step 10), the loop guard (step 10a) and the thread lock (step 19) are
    evaluated against the source at send time. A draft never holds a thread lock while it waits.
- `POST …/send` and approve answer `409 source_unavailable` with `details.reason`; a fire records it as
  `failure.reason`.

## 9. Storage

### 9.1 Mailbox tables

Added to the mailbox schema ([Data model §2](data-model.md#2-identitymailbox-durable-object-sqlite)). They
go into mailbox schema v1 while nothing is built; if M6 has shipped v1 when M29 starts, they are mailbox
schema v2, applied on wake with the same idempotent statements ([Design conventions §4, rule 6](index.md#4-durable-object-transactions)).

```sql
CREATE TABLE IF NOT EXISTS drafts (
  rowid              INTEGER PRIMARY KEY,
  id                 TEXT NOT NULL UNIQUE,                 -- drf_
  client_id          TEXT,                                 -- caller's idempotent-create key (§4.1)
  client_fingerprint TEXT,                                 -- sha256 of the canonical create body
  draft_kind         TEXT NOT NULL CHECK (draft_kind IN ('new','reply','reply_all','forward')),
  kind               TEXT NOT NULL CHECK (kind IN ('transactional','marketing','auto_reply')),
  status             TEXT NOT NULL CHECK (status IN ('editing','pending_approval','approved','scheduled',
                                                     'sending','sent','failed','rejected','canceled')),
  status_at          INTEGER NOT NULL,                     -- when it entered this status
  revision           INTEGER NOT NULL DEFAULT 1,
  index_version      INTEGER NOT NULL DEFAULT 1,           -- +1 on every write; guards the D1 index (§9.3)
  digest             TEXT,                                 -- hex sha256 of the current content (§2.4); NULL once cleared
  source_message_id  TEXT,                                 -- in_reply_to or forward_of (msg_); NULL for new
  thread_seq         INTEGER,                              -- the source's thread, or thread_id of a new draft;
                                                           -- no foreign key: erasure deletes the draft first (§13.2)
  from_address       TEXT,
  to_json            TEXT NOT NULL DEFAULT '[]',
  cc_json            TEXT NOT NULL DEFAULT '[]',
  bcc_json           TEXT NOT NULL DEFAULT '[]',
  resolved_json      TEXT,                                 -- reply kinds: {to, cc} as resolved for the last digest
  subject            TEXT,
  text               TEXT,                                 -- ≤ 524 288 bytes
  html               TEXT,                                 -- ≤ 1 048 576 bytes, as written
  headers_json       TEXT NOT NULL DEFAULT '{}',
  attachments_json   TEXT NOT NULL DEFAULT '[]',           -- [{id, filename, content_type, disposition,
                                                           --   content_id, size, sha256, source, r2_key, upload_id?}]
  include_attachments INTEGER,                             -- forward only
  calendar_json      TEXT,                                 -- new and reply drafts: the calendar field of the send
                                                           -- routes ({ics, attach_copy}, Calendar invitations § 5.1),
                                                           -- part of the digest; NULL otherwise
  unsubscribe_json   TEXT,
  consent_json       TEXT,
  labels_json        TEXT NOT NULL DEFAULT '[]',
  metadata_json      TEXT NOT NULL DEFAULT '{}',
  send_at            INTEGER,
  next_attempt_at    INTEGER,                              -- a scheduled fire waiting after a transient condition
  approval_json      TEXT NOT NULL DEFAULT '{"state":"none"}',  -- §2.1 approval
  send_json          TEXT,                                 -- §2.1 send request, with the authority
  claim_json         TEXT,                                 -- {token, claimed_at, prior_status, origin, attempt}
  rejection_json     TEXT,
  failure_json       TEXT,
  message_id         TEXT,                                 -- msg_ once sent
  sent_at            INTEGER,
  created_by_key_id  TEXT,
  updated_by_key_id  TEXT,
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS drafts_client  ON drafts(client_id) WHERE client_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_due     ON drafts(status, send_at);
CREATE INDEX IF NOT EXISTS drafts_source  ON drafts(source_message_id) WHERE source_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_thread  ON drafts(thread_seq) WHERE thread_seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_message ON drafts(message_id) WHERE message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_created ON drafts(created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS drafts_updated ON drafts(status, updated_at);

CREATE TABLE IF NOT EXISTS draft_sync (                     -- index rows waiting to be written to D1 (§9.3)
  draft_id  TEXT PRIMARY KEY,
  version   INTEGER NOT NULL,                              -- drafts.index_version it describes
  op        TEXT NOT NULL CHECK (op IN ('upsert','delete')),
  row_json  TEXT,                                          -- the draft_index row for an upsert: IDs, status, kind,
                                                           -- counts, times, key IDs; never content
  queued_at INTEGER NOT NULL
);
```

The row stays within the 1,900,000-byte budget of [Inbound › Storage caps](inbound.md#storage-caps):
`text` and `html` are bounded at write (§4.1), and every list is bounded (49 recipients per list, 32
attachments, 64 labels, 16 metadata keys, 16 KB of headers). The `drafts` table has 42 columns.

Changes to existing mailbox tables:

```sql
ALTER TABLE messages ADD COLUMN draft_id TEXT;      -- the draft it was sent from (§5.4); in v1: a column of CREATE TABLE
ALTER TABLE idempotency ADD COLUMN draft_id TEXT;   -- the draft a draft route's record, or a draft send's ledger row,
                                                    -- belongs to; erasure deletes by it (§13.2)
CREATE INDEX IF NOT EXISTS idempotency_draft ON idempotency(draft_id) WHERE draft_id IS NOT NULL;
-- idempotency.operation gains: draft_create, draft_send, draft_approve, draft_reject
```

New `meta` keys: `alarm:drafts` and `alarm:draft_index` (pending wake-ups, §6.2), `draft_index_backoff`
(consecutive failed index writes), and `draft_att:{attachment_id}` (an inline attachment written to R2 and
not yet committed, §7).

### 9.2 D1 `draft_index`

```sql
-- One row per draft that is pending_approval, scheduled or failed; written only by the draft's mailbox
-- (§9.3); read by GET /v1/tenants/{tenant_id}/drafts, the console's Overview and drafts page, the Notifier's
-- needs-a-person count and the drafts_due watchdog. It holds no mail content.
CREATE TABLE draft_index (
  draft_id            TEXT PRIMARY KEY,                    -- drf_
  tenant_id           TEXT NOT NULL REFERENCES tenants(id),
  identity_id         TEXT NOT NULL REFERENCES identities(id),
  status              TEXT NOT NULL CHECK (status IN ('pending_approval','scheduled','failed')),
  version             INTEGER NOT NULL,                    -- the mailbox's drafts.index_version
  revision            INTEGER NOT NULL,
  draft_kind          TEXT NOT NULL CHECK (draft_kind IN ('new','reply','reply_all','forward')),
  approval_reason     TEXT CHECK (approval_reason IN ('requested','new_recipients','external','always')),
  recipient_count     INTEGER NOT NULL,
  attachment_count    INTEGER NOT NULL,
  send_at             INTEGER,
  status_at           INTEGER NOT NULL,
  failure_reason      TEXT,
  requested_by_key_id TEXT,
  updated_at          INTEGER NOT NULL
);
CREATE INDEX draft_index_tenant ON draft_index(tenant_id, status, status_at, draft_id);
CREATE INDEX draft_index_identity ON draft_index(identity_id, status);
CREATE INDEX draft_index_due ON draft_index(send_at) WHERE status = 'scheduled';
```

It goes into `0001_init.sql` while nothing is built, as the service ledger's table did (M27); otherwise into
the next D1 migration, which only adds it.

### 9.3 Keeping the index in step

1. **Inside the draft's transaction.** Every transaction that changes a draft increments its
   `index_version` and, when the draft enters, leaves or changes inside the indexed statuses, writes the
   `draft_sync` row for it (`INSERT … ON CONFLICT (draft_id) DO UPDATE`, so only the newest state waits),
   with `op = 'upsert'` and the row, or `op = 'delete'`, and sets `alarm:draft_index = now`.
2. **The alarm writes D1.** Only the alarm writes the index, one run at a time, so writes for one draft
   never overtake each other. It reads up to 100 `draft_sync` rows, then one D1 batch:

   ```sql
   INSERT INTO draft_index (draft_id, tenant_id, identity_id, status, version, revision, draft_kind,
     approval_reason, recipient_count, attachment_count, send_at, status_at, failure_reason,
     requested_by_key_id, updated_at)
   VALUES (…)
   ON CONFLICT (draft_id) DO UPDATE SET status = excluded.status, version = excluded.version, …
   WHERE excluded.version > draft_index.version;          -- an older state never replaces a newer one
   DELETE FROM draft_index WHERE draft_id = ?1 AND tenant_id = ?2;   -- op = 'delete'
   ```

   then deletes each `draft_sync` row whose `version` is still the one it wrote. A failed batch sets
   `alarm:draft_index = now + d`, `d` = 30 s doubling per consecutive failure, at most 5 minutes
   (`meta.draft_index_backoff`), as the outbox does ([Webhooks › Dispatching](webhooks.md#dispatching-alarm-purpose-outbox)).
3. **What a lag means.** During a D1 outage the queue, the Overview and the watchdog read an older state.
   They decide nothing: an approve of a draft that is no longer pending answers what the mailbox says, and
   a watchdog poke to a mailbox with nothing due does nothing ([Q58](../edge-cases.md)). A `draft_sync` row
   older than 15 minutes fires `draft_index_stuck` ([§18](#18-observability)).

### 9.4 R2

| Key | Holds | Deleted |
|---|---|---|
| `t/{tenant_id}/i/{identity_id}/drafts/{draft_id}/a/{attachment_id}` | An inline draft attachment | When it is removed by `PATCH`, when the draft is sent or canceled (after commit), by erasure and by the retention purge of its draft; the orphan sweep removes uncommitted ones. Under the identity's prefix, so identity and tenant erasure sweep it ([Privacy §6.5](privacy.md#65-identity-scope-fr-idn-4), [§6.6](privacy.md#66-tenant-scope)), and the R2 backup copy follows `BLOBS` |

## 10. Concurrency and idempotency

| Race or repeat | What decides | Result |
|---|---|---|
| Two `PATCH`es with the same `If-Match` | The mailbox runs one transaction at a time and re-reads the revision inside it | One succeeds; the other `409 draft_revision_mismatch` |
| Two `PATCH`es without `If-Match` | Last writer wins, merged on the newer revision | Both succeed; revision rises by two |
| An edit and an approval of the old revision | The approval's `revision` (and `digest`) | `409 draft_revision_mismatch`; or, if the approval committed first, the edit drops it and asks again (T12) |
| A send by the API and the alarm fire, or two API sends with different keys | The claim (§5.3) | One message; the other `409 draft_not_sendable` (`retryable: true`) or skipped |
| The same `Idempotency-Key` repeated | The mailbox `idempotency` table and `in_flight` | Replay with `deduplicated: true`, or `409 request_in_progress`; another body, operation or draft `409 idempotency_conflict` |
| `client_id` repeated, also concurrently | `drafts_client` unique index, re-checked in the transaction | One draft; the other `200` with it (same body) or `409 client_id_conflict` |
| `DELETE` or reject while a send composes | The accept transaction's draft check (§5.3 rule 3) | The send is abandoned with nothing stored, or, if it committed first, `DELETE` and reject answer `409` |
| An alarm delivered twice, or re-run after a crash | The claim and the `scheduled`/due re-check | At most one message |
| An erasure while a send composes | `erased_ids` and the draft check | The draft is gone; the accept transaction finds no draft and abandons ([I10](../edge-cases.md)) |
| The index written out of order | Only the alarm writes; `version` guard | The index ends at the newest state |

## 11. Failure handling

| Failure | Behaviour |
|---|---|
| D1 unavailable when a route resolves the identity | `503 unavailable`, as on every route ([Security § 5.2](security.md#52-order-of-checks)); nothing changes |
| D1 unavailable after the mailbox committed (the index or the audit row) | The route answers normally; the index row waits in `draft_sync` (§9.3) and the audit row with its event in the outbox |
| D1 unavailable when loading the send context; R2 unavailable when reading an attachment; `TenantQuota` unavailable | `POST …/send`: `503 unavailable` and the draft unchanged (retry with the same key); approve with `send: true`: the approval is recorded and the draft is `scheduled` for an attempt 30 seconds later (§4.6), so the decision is not lost; the fire: transient (§6.4) |
| R2 write fails for an inline attachment | `503 unavailable`; nothing committed; the markers name any partial object for the sweep |
| The Worker dies between the claim and the accept transaction | §5.3 rule 5: no message; resolved after 2 minutes |
| The Worker dies after the accept transaction, before the response | The draft is `sent`; the caller's retry with the same key replays it |
| The alarm's retries are exhausted | §6.5 catch-up |
| The mailbox is overloaded or its call times out | `503 unavailable` or `504 timeout` (retryable); for a send the timeout comes before the message is queued, so a retry with the same key is safe |
| The outbox dispatch fails | Events and audit rows wait in the outbox, as every event does |
| A transport fails after the message is accepted | The message's own outcome (`uncertain`, `failed` …); the draft stays `sent` |

## 12. Security and visibility

- **Key levels.** `drafts:read` and `drafts:write` at every level (an identity key only for its own
  identity); `drafts:approve` only for platform, partner and tenant keys; the tenant queue for platform,
  partner (its own tenants) and tenant keys. The nine routes join `it::security::cross_tenant_matrix`: a
  draft of another tenant, or of a tenant another partner created, answers `404 draft_not_found` exactly
  as a missing one ([Q53](../edge-cases.md)).
- **Approval is a decision reserved for people** on Pylota Mail Cloud (`PM_QUARANTINE_KEY_RELEASE=off`),
  unless the tenant's `drafts.key_approval` is `true`, which only a platform key or the tenant's own
  partner key can set (class *platform or own partner*, like `quarantine.key_release`; a workspace writer
  gets `403 scope_denied` with `details.reason = "not_writable"`) ([Q17](../edge-cases.md)).
  `auth::people::key_may_decide` gains a `Decision` argument; for `ApproveDraft` it reads
  `drafts.key_approval` where the other decisions read `quarantine.key_release`, and, as for those, the
  opt-in admits only a platform key or the tenant's own partner key, never a tenant key.
- **No self-approval** by the key that wrote the revision; an identity key cannot approve at all.
- **Approval binds to content** (§2.4): an approval cannot be moved to other content, other recipients
  (reply recipients included) or another sender identity.
- **Untrusted content.** Draft text is agent-written. The console shows it escaped or in the sandboxed
  frame ([§16](#16-console)); notes, reasons and `on_behalf_of` are escaped too. MCP results mark draft
  fields as untrusted like mail fields.
- **The system identity** has no drafts: every draft route on it answers `404 identity_not_found`, as
  every tenant-facing route does ([Q52](../edge-cases.md)).
- **No content outside the mailbox.** D1 `draft_index`, audit rows, events, logs and the idempotency
  records hold IDs, counts, codes and times only. Logs follow [Security § 12](security.md#12-logging-rules).

## 13. Privacy

### 13.1 Retention

Drafts are working copies of mail. They are kept only as long as they are useful, then deleted by the
mailbox's daily maintenance (the `Hygiene` operation of the tenant retention job, [Privacy §5.2](privacy.md#52-steps-of-a-tenant-retention-job)
step 5, which passes the tenant's `retention.message_days`). Pylota Mail choices:

| Status | Rule | Why |
|---|---|---|
| `pending_approval` | Becomes `rejected` with reason `expired` 7 days after `approval.requested_at`, or at `send_at` + 1 hour when that is later (T10, `draft.rejected`); checked by the `drafts` alarm purpose | An undecided request should not wait forever. Seven days matches the service ledger's pending lifetime; a draft scheduled further ahead waits for its own time |
| `editing`, `approved`, `failed`, `rejected` | Deleted 30 days after `updated_at` | Thirty days equals the idempotency window and the default `retention.events_days`, so a client replaying a request or an event within those windows still finds the draft. An abandoned draft's content is not kept beyond that |
| `sent`, `canceled` | Content cleared at the transition; the record deleted 30 days after `sent_at` or the cancel | The message holds the content; the record links the message to its approval for a month, and the message keeps `draft_id` after that |
| `scheduled` | Never deleted by retention before its fire; the late limit ends it | – |
| Any | When `retention.message_days` is set and shorter than 30 days, or the identity's `retention.message_hours` ([Parity additions](parity-additions.md)) is shorter still, the shortest replaces the 30 days | A draft must not outlive the tenant's or the identity's own retention of mail |
| Any, in a thread under an active legal hold | Kept while the hold lasts ([Privacy §8](privacy.md#8-legal-holds)) | FR-PRV-4 |

Each run that deletes drafts adds `drafts_deleted` to the step's `retention.purge` audit row. A `new`
draft without `thread_id` belongs to no thread, so no legal hold covers it.

### 13.2 Erasure

Every scope deletes drafts, with the counts in the receipt (`drafts_deleted`, and their inline objects in
`r2_objects_deleted` and their outbox events in `events_deleted`), and skips drafts in a held thread
([Q46](../edge-cases.md), [Q47](../edge-cases.md)):

| Scope | Drafts deleted | Where |
|---|---|---|
| Message `M` | Drafts with `source_message_id = M` (replies to and forwards of it), the record of the draft that became `M` (`message_id = M`), and the drafts of a thread the erasure leaves empty, which is deleted ([Q64](../edge-cases.md)) | `ErasurePlan` lists them with `M`; `EraseRows` deletes them ([Privacy §6.2](privacy.md#62-what-eraserows-deletes)) |
| Thread `T` | Drafts with `thread_seq = T`, and drafts whose source or sent message is in `T` | As above |
| Counterparty `A` | Drafts with `A` in `to_json`, `cc_json`, `bcc_json` or `resolved_json`, and drafts whose source or sent message matches `A` (and is therefore erased) | `ErasurePlan { Counterparty }` (and `CounterpartyHash` for a re-application after a restore, which hashes each draft address the same way) |
| Identity | Every draft, by `WipeAll` (counted by `CountAll`) or, with holds, by target `All` | Identity step 3; the `drafts/` objects go with the identity's R2 prefix; `draft_index` rows of the identity in step 4 `scrub_control_plane` |
| Tenant | As identity, for every identity | Step 4; `draft_index` in step 5 `delete_d1_rows` (with `tenant_id`) |

`EraseRows` gains, per erased draft, in the same transaction as the messages: delete the draft row, its
`draft_sync` row (and write a `delete` one, so the index row goes even if the scrub step races), its
`idempotency` rows (`draft_id`), and its outbox rows (`data.draft.id`); its inline R2 objects are listed in
the plan and deleted before `EraseRows`, like message objects. The `scrub_control_plane` step of message,
thread and counterparty scope also deletes `draft_index` rows for the erased draft IDs. The keyword probe
is unchanged (drafts are not in the FTS index); the object probe lists each erased draft's `drafts/{id}/`
prefix.

**Legal holds** keep a draft whose `thread_seq`, source message or sent message is in a held thread, in
every scope and in retention; the thread's entry in `receipt.held` covers it.

**What drafts never hold**: anything outside the mailbox and R2. The D1 index row, audit rows and events
hold IDs, counts and codes, and are deleted or kept by their own existing rules.

## 14. Billing and limits

**Billing.** A draft takes no hold while it is written, waits for approval or is scheduled. The `sends`
hold is taken at the actual send, by `submit`'s step 18, exactly as for any send, and settled at the
transport outcome (FR-BILL-4, FR-BILL-5). A scheduled send that meets `402 billing_limit` becomes `failed`
with `billing_limit`; `TenantQuota` emits `billing.limit_reached` once per feature and period as today, and
the usage alerts at 80% and 100% have already gone to the owner and admins through the `Notifier`
([Notifications §4](notifications.md#4-usage-alerts)), so no new notification path exists. A refused send
records no idempotency row, so after an upgrade an explicit resend with the same revision succeeds
([Q49](../edge-cases.md), [Q50](../edge-cases.md)). Drafts are not metered and do not count against
`storage_gb` beyond what the mailbox's SQLite size and R2 objects already measure.

**Limits** (to be copied into [Limits](../../reference/limits.md)):

| Limit | Value | Source |
|---|---|---|
| Drafts per identity, not counting `sent` and `canceled` records | 1,000; then `422 draft_limit_reached` | Pylota Mail |
| `send_at` | At least 60 seconds and at most 90 days after the request | Pylota Mail (§6.1) |
| Late limit of a scheduled send | 1 hour after `send_at`; then `failed` with `send_window_missed` | Pylota Mail (§6.4) |
| Attempts after an interrupted send | 3; then `failed` with `send_interrupted` | Pylota Mail |
| Scheduled sends per alarm run of one mailbox | 10, then the alarm re-arms at once | Pylota Mail |
| Pending approval | 7 days after it was requested, or `send_at` + 1 hour when later | Pylota Mail |
| Draft retention | 30 days after the last change (or `retention.message_days` when shorter); `sent` and `canceled` records 30 days | Pylota Mail |
| `text`, `html` | 524,288 and 1,048,576 bytes | Pylota Mail ([Inbound › Storage caps](inbound.md#storage-caps)) |
| Attachments | 32 per draft; inline bytes within the 7 MiB request body | Pylota Mail |
| Recipients, subject, headers, labels, metadata | As for sends | [Limits › Mail](../../reference/limits.md#mail) |
| `note`, `reason` | 500 characters | Pylota Mail |
| `on_behalf_of` | 1–128 printable ASCII characters, no `@` | Pylota Mail |
| `client_id` | 1–255 printable ASCII characters without `@`, unique per identity | Pylota Mail |
| Durable Object alarm | One per object; at-least-once; 6 retries from 2 seconds | [Alarms](https://developers.cloudflare.com/durable-objects/api/alarms/), read 2026-10-17 |
| Alarm handler wall time | 15 minutes | [Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17 |

## 15. Events

Five mailbox events, appended to the identity's outbox in the transaction of the change
([Webhooks › Appending](webhooks.md#appending-inside-the-state-change-transaction)), so each carries the
identity's `sequence` and reaches the tenant's, its partner's and platform endpoints through the usual
fan-out, with identity filters applied, and the event feed with the same `evt_` ID
([Event feed](event-feed.md)). They are **thin**: no subject, body, address, filename, note or reason
text. So an endpoint that subscribes to them needs only `webhooks:manage`: the rule of
[J29](../edge-cases.md) for mail events does not apply ([Q51](../edge-cases.md)).

`data.draft` is the **draft summary**:

```json
{ "id": "drf_01JA…", "identity_id": "idn_01J9…", "draft_kind": "new", "kind": "transactional",
  "status": "pending_approval", "revision": 3, "digest": "9f2c41ab…", "recipient_count": 1,
  "attachment_count": 2, "send_at": null, "in_reply_to": null, "forward_of": null, "thread_id": null,
  "client_id": "pm:v1:draft:apv_8812" }
```

| Event | When | `data` |
|---|---|---|
| `draft.approval_requested` | A draft entered `pending_approval`, or was edited while in it, or a send found approval needed (T1, T2, T3, T11, T12, §5.2) | `draft`, `reason` (`requested`, `new_recipients`, `external`, `always`), `requested_by_key_id` |
| `draft.approved` | An approver approved a revision (T6, T7, T8) | `draft`, `send` (boolean), `approved_by_key_id` or `approved_by_user_id` (the other `null`), `on_behalf_of` |
| `draft.rejected` | An approver rejected it, or its approval request expired (T9, T10) | `draft`, `reason` (`operator` or `expired`), `rejected_by_key_id` or `rejected_by_user_id`, `on_behalf_of` |
| `draft.sent` | The draft became a message (T4, T7, T11, T14) | `draft`, `message_id`, `thread_id`, `message_status` (`queued`, or `suppressed` when every recipient was suppressed) |
| `draft.failed` | A send it did not answer synchronously ended without a message (T7, T11, T15) | `draft`, `reason` (the `failure.reason` codes of [§6.3](#63-the-fire)), `attempts` |

`client_id` is the caller's own key, which it chose to contain no personal data (it is in the summary so a
consumer such as Pylota maps an event to its record without a read); `on_behalf_of` is the caller's opaque
user ID. The resulting message emits its own `message.*` events as any message does, and its summary
carries `draft_id`. The outbox dispatch writes the audit rows of `draft.approved` and `draft.rejected`
(§4.6).

Builders: `draft_summary(row)` and `draft_event(row, extra)` in `webhooks/payloads.rs`, with golden-file
tests and the test that no body field appears in `data`.

## 16. Console

The console reviews and decides drafts; it does not write them (no webmail, [PRD §4](../prd.md#non-goals-v10)).
Every page works without JavaScript: each action is a `<form method="post">` with the CSRF token
([Console › CSRF](console.md#csrf)).

| Path | Screen | Who |
|---|---|---|
| `/console/drafts` | The workspace's drafts that need attention, from D1 `draft_index`: tabs **Awaiting approval** (oldest first), **Scheduled** (by `send_at`) and **Failed**; each row shows the inbox address, kind, recipient count, attachment count, approval reason, requester key name, `send_at` and age; filters by inbox; for a page of rows the screen reads each identity's drafts once (`ListDraftsByIds`) to show recipients and subject | View: owner, admin, member. Viewers get `403` |
| `/console/inboxes/{idn}/drafts` | One inbox's drafts in every status, from its mailbox | As above |
| `/console/inboxes/{idn}/drafts/{drf}` | One draft: recipients (resolved now for replies), subject, text view (escaped), HTML on request in the sandboxed frame, attachments (downloaded from the console host with the attachment serving headers), the source message link when the session may see it, the approval, send, rejection and failure details, and for a `sent` draft the message link | As above; the forms below for owner and admin |

**Forms** on a draft (owner and admin; the session principal holds `drafts:approve` and `messages:send`):

- **Approve** with three choices: *Approve and send now*, *Approve and send at {send_at}* (shown when the
  draft has a future `send_at`), *Approve only*; an optional note; hidden `revision` and `digest` fields
  holding what the page showed, so a draft edited after the page was rendered is refused with "This draft
  changed since you opened it" and the page shown again. Approving is a **sensitive action**: it needs a
  sign-in within the last 10 minutes ([Console › Re-authentication](console.md#re-authentication)), as
  releasing quarantine does (FR-CON-5, FR-CON-6), and writes `draft.approve` with `actor_user_id` and
  `via: "console"`. When `send_at` has passed, the button reads "The scheduled time has passed; approving
  sends it now".
- **Reject** with a required reason (500 characters); not sensitive, because it only keeps mail from
  leaving; audit `draft.reject`.
- **Cancel** (owner and admin, who hold `drafts:write`); not sensitive.

There is no edit form: a person who wants different content rejects with a reason, and the writer revises.

**Overview.** "Needs a person" ([Cloud sign-up §8](cloud-signup.md#8-the-overview-the-screen-people-land-on))
gains two items, each linking to `/console/drafts`:

- **Drafts awaiting approval**: the count and the five oldest, with inbox, kind and recipient count;
- **Scheduled sends that failed**: the count, with the most common `failure.reason`.

Both come from two D1 queries on `draft_index`:

```sql
SELECT status, COUNT(*) FROM draft_index
WHERE tenant_id = ?1 AND status IN ('pending_approval','failed') GROUP BY status;
SELECT draft_id, identity_id, draft_kind, recipient_count, status_at FROM draft_index
WHERE tenant_id = ?1 AND status = 'pending_approval' ORDER BY status_at, draft_id LIMIT 5;
```

The daily "needs a person" email ([Notifications §1](notifications.md#1-kinds)) reads the same counts and
holds no content. Owner, admin and member sessions gain `drafts:read`; owner and admin sessions also gain
`drafts:write` and `drafts:approve`, so the keys they mint for agents can carry `drafts:write`
([Console › Roles](console.md#roles)).

## 17. MCP and CLI

### 17.1 MCP tools

| Tool | REST endpoint | Permission | Rate limit |
|---|---|---|---|
| `mail_create_draft` | `POST /v1/identities/{id}/drafts` (`idempotency_key` optional, sent as `Idempotency-Key`) | `drafts:write` | – (`RL_API` only) |
| `mail_update_draft` | `PATCH …/drafts/{draft_id}` (`etag` argument sent as `If-Match`) | `drafts:write` | – |
| `mail_list_drafts` | `GET /v1/identities/{id}/drafts` | `drafts:read` | – |
| `mail_get_draft` | `GET …/drafts/{draft_id}` | `drafts:read` | – |
| `mail_send_draft` | `POST …/drafts/{draft_id}/send` (`idempotency_key` required) | `messages:send` and `drafts:read` (a key with only one never sees the tool) | `RL_SEND` |
| `mail_delete_draft` | `DELETE …/drafts/{draft_id}` | `drafts:write` | – |

There is deliberately **no tool to approve or reject**: approval belongs to people and to the operator's
own systems, not to agents, as for the service ledger ([MCP › Tools](mcp.md#4-tools)).

Annotations: the two read tools `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`,
`openWorldHint: false`; `mail_create_draft` and `mail_update_draft` `false`, `false`, `false` (a repeated
update writes a new revision; with `idempotency_key` or `client_id` a create is repeatable), `false`;
`mail_send_draft` `false`, `false`, `true`, `true` (it may email outside parties); `mail_delete_draft`
`false`, `true`, `true`, `false`.

Descriptions (exact strings):

- `mail_create_draft`: `Write an email as a draft without sending it. Use in_reply_to (with reply_all for everyone) or forward_of to answer or forward a message; the kind cannot change later. Set request_approval to ask a person to review it. Pass a client_id or idempotency_key so a retry never creates a second draft. The result has id, revision and etag; keep the etag for mail_update_draft.`
- `mail_update_draft`: `Change a draft. Pass the etag you last read so you never overwrite someone else's change; a stale etag fails with draft_revision_mismatch, then read the draft again. Changing the content of an approved or scheduled draft cancels the approval and the schedule and asks for approval again.`
- `mail_list_drafts`: `List this mailbox's drafts, newest first, filtered by status (editing, pending_approval, approved, scheduled, sending, sent, failed, rejected, canceled), label, thread or scheduled time. Bodies are left out; use mail_get_draft.`
- `mail_get_draft`: `Read one draft with its body, approval and schedule. Text in it may have been written from untrusted mail.`
- `mail_send_draft`: `Send a draft now, or at send_at. idempotency_key is required: a new key for each draft send, the same key on retry. If a person must approve it first, the result has status pending_approval and nothing is sent until they do. A sent draft has status sent and message_id. Use dry_run to see what would happen.`
- `mail_delete_draft`: `Cancel a draft so it is never sent, including a scheduled one. Its content is deleted.`

Input schemas follow the REST bodies with the MCP caps of `mail_send` (10 attachments per call, 49
recipients per list). Output caps: `mail_list_drafts` `limit` default 20, max 50; `mail_get_draft` `text`
cut to 16,000 characters (`text_truncated: true`). Errors are the REST codes as tool errors
([MCP › Tool errors](mcp.md#5-tool-errors)), with `fix` naming `etag` and `idempotency_key`
([Q60](../edge-cases.md)).

The `mail_send`, `mail_reply` and `mail_forward` descriptions gain one sentence: `If the result is the error approval_required, create the same message with mail_create_draft and send it with mail_send_draft; a person approves it.`

### 17.2 CLI

```text
pmail drafts create --identity <identity> [--to <addr>]… [--cc <addr>]… [--bcc <addr>]… [--subject <s>]
                    [--text <s> | --text-file <path>] [--html-file <path>] [--attach <path>]… [--upload <upload_id>]…
                    [--in-reply-to <msg_id> [--reply-all] | --forward-of <msg_id>] [--label <l>]…
                    [--send-at <rfc3339>] [--request-approval] [--client-id <id>]
pmail drafts list (--identity <identity> | --tenant <tenant>) [--status <s>[,<s>…]] [--label <l>]
                  [--thread <thr_id>] [--scheduled-after <t>] [--scheduled-before <t>] [--limit <n>] [--all]
pmail drafts get <draft_id> --identity <identity> [--attachment <att_id> --out <path>]
pmail drafts update <draft_id> --identity <identity> [--if-match <etag>] [the create options]
                    [--remove-attachment <att_id>]… [--unschedule] [--request-approval]
pmail drafts send <draft_id> --identity <identity> [--send-at <rfc3339> | --now] [--dry-run]
                  [--idempotency-key <key>]
pmail drafts approve <draft_id> --identity <identity> --revision <n> [--digest <hex>]
                     (--send | --no-send) [--note <text>] [--on-behalf-of <id>]
pmail drafts reject <draft_id> --identity <identity> --reason <text> [--revision <n>]
pmail drafts delete <draft_id> --identity <identity> [--yes]
```

`pmail drafts send` without `--idempotency-key` derives one from the draft ID and revision
(`cli:{draft_id}:{revision}`), so re-running the same command never sends twice. `list --tenant` reads the
tenant's queue. Exit codes follow [CLI › Exit codes](../../reference/cli.md): `approve` where people decide
exits 4 (`403 permission_denied`, `person_required`); a stale revision exits 6 (`409`); the draft limit
exits 7 (`422`).

## 18. Observability

**Metrics** ([Observability §3.2](observability.md#32-catalogue)):

| Metric | Kind | Labels | Emitted by |
|---|---|---|---|
| `drafts_written_total` | counter | `op` (`create`, `update`, `delete`) | mailbox |
| `draft_sends_total` | counter | `origin` (`api`, `approval`, `console`, `alarm`), `outcome` (`sent`, `pending_approval`, `scheduled`, `refused`, `failed`, `transient`) | mailbox |
| `draft_send_failures_total` | counter | `reason` (the `failure.reason` codes) | mailbox |
| `draft_decisions_total` | counter | `decision` (`approve`, `reject`, `expire`), `actor` (`key`, `person`, `system`), `send` | mailbox |
| `draft_approval_rerequested_total` | counter | `cause` (`edit`, `digest_changed`, `approver_invalid`) | mailbox |
| `draft_fire_lag_ms` | observation | – | mailbox: the fire's start minus `send_at` |
| `draft_claims_expired_total` | counter | `origin` | mailbox (§5.3 rule 5) |
| `drafts_alarm_missed_total` | counter | – | `crons/drafts_due.rs` |
| `draft_index_write_failures_total` | counter | – | mailbox (§9.3) |
| `send_approval_required_total` | counter | `level` | `fetch` (step 16a refusals) |

**Alerts** ([Observability §5.3](observability.md#53-alert-list)):

| Alert | Class | Condition | Severity | Runbook |
|---|---|---|---|---|
| `drafts_overdue` | B | The watchdog finds a `scheduled` draft more than 10 minutes past `send_at` in two consecutive minutes | page | Drafts overdue (below) |
| `draft_index_stuck` | B | A mailbox reports a `draft_sync` row older than 15 minutes (the size check of its next write reports it, as `mailbox_size` is reported) | ticket | Check D1 health and `draft_index_write_failures_total`; the rows retry with back-off |
| `draft_send_interrupted` | A | `draft_send_failures_total{reason=send_interrupted}` > 0 over 1 hour | ticket | Read the logs of the claims that expired (`draft_claims_expired_total`); a repeating crash in composition is a bug |

**Runbook: Drafts overdue.** (1) Read `drafts_alarm_missed_total` and `draft_fire_lag_ms`: a missed alarm
that the watchdog repaired raises only the first. (2) If the watchdog's pokes fail, the mailbox is erroring:
read its logs by `identity_id`. (3) A draft past its late limit is already `failed` with
`send_window_missed` and in the workspace's "Needs a person"; nothing sends it without an explicit act.

**Logs.** `draft_write`, `draft_send`, `draft_decide`, `draft_fire` and `draft_index_sync` events with IDs,
status, revision, origin and codes; never content, addresses or notes
([Observability §2](observability.md#2-structured-logs)).

## 19. Pylota agentic OS

**Who uses drafts, with which key.** Pylota keeps its key hierarchy (integration spec §6) and adds one key
kind and three permissions:

| Pylota agent or path | Key | Draft permissions | Why |
|---|---|---|---|
| The public-webchat coordinator, and any path whose input is untrusted (a renter's chat turn, mail-steered specialists on first contact) | **K8**, a new identity key per operator identity that needs it, minted by K1 | `drafts:write` only; no `messages:send`, no `messages:read` | It can write mail and can never send it, so every message it writes waits for an operator (pattern 2). Without `messages:read` it cannot create reply drafts to mail it cannot read (`404 message_not_found`) or read the mailbox |
| Specialists that send for an operator (K3) | K3 | none added | Under the operator tenant's `drafts.require_approval: "new_recipients"`, a direct send to a new address is refused with `403 approval_required`; the capability route then turns it into a draft (pattern 1) |
| Pylota's server, for operator decisions | K1 (partner key) | add `drafts:read`, `drafts:approve` (it already holds `messages:send`) | Reads drafts to show them in Pylota's app and approves or rejects for the operator |

**Policy Pylota sets** on each operator tenant at creation (spec §4.3), with K1: `drafts.key_approval: true`
(which only a platform key or the tenant's own partner key can set) and `drafts.require_approval:
"new_recipients"`. Platform, marketing and auth tenants keep `never`: their identities are backend senders.

**Mapping onto Pylota's approval gate.** The gate (tool-risk registry; enforcement is the feature-flag row)
stays Pylota's decision point; Pylota Mail is the server-side backstop that holds even if the gate's flag is
off or a bug skips it:

1. A gated mail tool, or a K3 send answered `approval_required`, creates the draft through Pylota's
   capability route with the identity's K8 (or K3) key and `client_id: "pm:v1:draft:{pendingActionId}"`,
   and `request_approval: true` (K8) or `POST …/send` (K3). The pending action row stores `draft_id`,
   `identity_id`, `revision` and `digest`, never the content.
2. Pylota's app shows the draft to the operator by reading it with K1 (`GET …/drafts/{draft_id}`), as
   untrusted text.
3. The operator approves in Pylota's app. Pylota's own freshness check (`approval_stale` fails closed)
   runs first. Then Pylota's server calls `POST …/drafts/{draft_id}/approve` with K1, `revision` and
   `digest` from the pending action, `send: true`, `on_behalf_of` = the operator's Pylota user ID, and
   `Idempotency-Key: pm:v1:draft-approve:{pendingActionId}`. A `409 draft_revision_mismatch` means the
   agent changed the draft after the operator saw it: Pylota shows the new revision. A rejection calls
   `…/reject` with the operator's reason and `on_behalf_of`, which the agent reads on its next turn.
4. Pylota's partner webhook endpoint adds `draft.approval_requested`, `draft.approved`, `draft.rejected`,
   `draft.sent` and `draft.failed` to its explicit list (they carry no content, so no extra permission is
   needed). `draft.sent` closes the pending action with `message_id`, and the message's `message.*`
   events then update `mail_outbound_messages` as today (spec §8.3; the summary's `draft_id` links them);
   `draft.failed` shows "Not sent: {reason}" with a Retry that calls `POST …/send` (K1 or K3) as an
   explicit act; `draft.rejected` with `reason: expired` closes the pending action as expired.
5. **First contact (spec §7.7).** Rule 4 replaces the send-allow entries Pylota writes for approved
   recipients (`operator_approved`, `operator_approved_reply`, `operator_approved_seller`): an approved
   draft reaches its recipients for that send, and later mail is known by `outbound_count`, without a
   tenant-wide entry that outlives the decision. Fleet acquisition's first message to a seller becomes a
   draft with `client_id: "acq-{sha256}"` and its send `Idempotency-Key: "acq-{sha256}:send"`, approved by
   the operator, instead of the `first_contact_needs_approval` tool error.

**Where failures surface in Pylota.** `403 approval_required` on a direct send (the route converts it, so it
never reaches the operator); `draft.failed` in the conversation bubble and the admin outbound page, by
reason; `403 permission_denied` with `person_required` on approve means `drafts.key_approval` is missing:
the provisioner re-applies it with K1 and pages ops, as it does for `quarantine.key_release` (spec §8.6);
`409 draft_not_pending` on approve after an operator took too long means the request expired after 7 days.

**Scheduled sends.** Pylota may schedule booking-time mail (a pick-up reminder created with the booking,
`send_at` the day before) and must `DELETE` the draft when the booking changes or is cancelled; Kairos
remains the source of the booking's time. Pylota's own pg-boss schedules stay for anything beyond 90 days.

**What Pylota's integration spec must add** (for the spec's owner):

1. §6: key K8 (identity, `drafts:write`), minted by K1 for the identities whose untrusted paths draft; K1
   gains `drafts:read` and `drafts:approve`; the mint and rotation rules of K3 apply to K8.
2. §4.3: tenant creation sets `drafts.key_approval: true` and `drafts.require_approval: "new_recipients"`
   on operator tenants, `never` on Pylota-owned tenants.
3. §7: the router contract learns `403 approval_required` (convert to a draft, never retry with a new
   key); a `mail_drafts` table (pending action ↔ `draft_id`, `revision`, `digest`, status, `message_id`; no
   content); the draft keys `pm:v1:draft:{pendingActionId}` and `pm:v1:draft-approve:{pendingActionId}`.
4. §7.7: replace the `operator_approved*` send-allow entries with approved drafts; keep
   `counterparty_on_file` entries, which are server facts rather than approvals.
5. §8.1: add the five `draft.*` events to the partner endpoint's list; §8.3: the `draft.*` rows of the
   outcome table.
6. The approval-gate design: a gated mail tool writes a draft instead of holding the send in Pylota, and
   the public-webchat coordinator's roster gets a draft-only mail tool, if it gets one at all.
7. Tests: a K8 key can never send (`messages:send` absent); an approval with a stale revision is shown
   again; `draft.sent` closes the pending action; a renter's erasure removes drafts addressed to them
   (counterparty scope).

## 20. Functional requirements

To be added to [PRD § 6](../prd.md#6-functional-requirements) as § 6.16 "Drafts, approval and scheduled
send":

- **FR-DRF-1** The service **must** store drafts in the identity's mailbox, with create, list, get,
  update and delete through the API (`drafts:read`, `drafts:write`). A create with a `client_id` the
  identity has already used **must** return the existing draft, or `409 client_id_conflict` if the
  request differs. A draft's kind (`new`, `reply`, `reply_all`, `forward`) **must** be fixed at creation,
  and a field its kind does not take **must** be refused. Reply and forward drafts **must** name a source
  message the creating key can see, never `hidden` or `throttled` mail, and `quarantined` or `screening`
  mail only with `quarantine:review` ([Drafts](drafts.md)).
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
  known (FR-OUT-13 rules 1–3) nor internal to the workspace and `external` meaning a recipient that is
  not internal, and an identity's `send_policy.require_approval` **may** only make it stricter. It
  **must** apply to every send of the tenant's identities except the system identity's: an ordinary send,
  reply, reply-all or forward that needs approval **must** be refused with `403 approval_required` before
  anything is stored. It is a guard field (FR-TEN-4).
- **FR-DRF-5** Approving and rejecting **must** need `drafts:approve`, which identity keys **must never**
  hold; approving with `send: true` **must** also need `messages:send`. On a deployment where only people
  may release quarantine (FR-CON-6), an API key **must** approve only on a tenant whose policy has
  `drafts.key_approval: true`, which only a platform key or the tenant's own partner key can set, and
  then only if it is a platform key or that partner key, never a tenant key. A key
  **must not** approve a revision it wrote. An approval **must** bind to the digest of the content
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
  draft; at send the draft's copies **must** be copied into the message's own attachment objects with their
  SHA-256 checked.
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

**Changes to existing requirements**: FR-OUT-3 gains "the approval requirement of FR-DRF-4" before the
caps; FR-OUT-13 gains rule 4 (FR-DRF-6); FR-CON-5's list of sensitive actions gains "approving a draft";
FR-CON-12's and FR-CON-15's "needs a person" gain "drafts awaiting approval and scheduled sends that
failed"; FR-PRV-3's receipt gains `drafts_deleted`. The text is in the [PRD](../prd.md).

## 21. Edge cases

To be added to [Edge cases](../edge-cases.md) as section "Q · Drafts, approval and scheduled send".

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| Q1 | A create repeats a `client_id`, also two creates racing with it | Same body: `200` with the existing draft and `Idempotent-Replayed: true`; another body: `409 client_id_conflict` with `details.draft_id`; two racing creates make one draft. A `client_id` with `@` is `400 invalid_request` | S+I | `it::drafts::q1_client_id_idempotent_create` |
| Q2 | `in_reply_to` with `forward_of`; `reply_all` without `in_reply_to`; a reply draft with `to`, `cc`, `bcc`, `subject` or `from_address`; a forward with `subject`; `kind: marketing` on a reply; `calendar` on a reply-all or forward draft | `400 invalid_request` with `details.errors[].path`; nothing stored | S | `core::drafts::kind_field_rules` |
| Q3 | A reply or forward draft whose source is `hidden`, `throttled`, absent, another identity's, or `quarantined` or `screening` without `quarantine:review`; a `thread_id` not in this mailbox; reading a draft of a quarantined source without `quarantine:review` | `404 message_not_found` or `404 thread_not_found` at create or update; such a draft is listed and read only with `quarantine:review`, otherwise `404 draft_not_found` | S | `it::drafts::q3_source_visibility` |
| Q4 | A `PATCH` sends `in_reply_to`, `forward_of` or `reply_all` | `400 invalid_request`: the kind is fixed at creation | S | `core::drafts::kind_field_rules` |
| Q5 | A draft body over 7 MiB; `text` over 524,288 bytes or `html` over 1,048,576; 33 attachments; bad base64; a header outside the allowed set; more recipients than `max_recipients`; own parts over the composed limit with `large_attachments: refuse` | `413 payload_too_large`; `400 invalid_request`; `400 header_not_allowed`; `400 too_many_recipients`; `413 message_too_large`; nothing stored and no R2 object left | S | `it::drafts::q5_oversize_and_bounds` |
| Q6 | An upload reference that does not exist, belongs to another identity or tenant, is not yet `ready` or has expired | `404 upload_not_found` (also for another identity's or tenant's upload, so nothing is revealed), `409 upload_not_ready` or `410 upload_expired`; nothing stored | S | `it::drafts::q6_upload_references` |
| Q7 | `send_at` 59 seconds ahead, 90 days and 1 ms ahead, in the past, unparseable, or with a non-UTC offset | The first three and the fourth: `400 invalid_request`, path `send_at`, `details.reason` `too_soon` or `too_far` where it applies; an offset is converted to UTC and accepted inside the window | S | `core::drafts::etag_and_send_at`, `it::drafts::q5_oversize_and_bounds` |
| Q8 | The 1,001st draft of an identity (not counting `sent` and `canceled`), also two creates racing for the last place | `422 draft_limit_reached` with `details.limit`; exactly one of the racing creates succeeds | S | `it::drafts::q8_draft_limit` |
| Q9 | Two writers edit one draft: both with the same `If-Match`, and both without | Same `If-Match`: one `200`, the other `409 draft_revision_mismatch` with `details.current_revision` and `current_etag`, nothing written; without: both succeed, `revision` rises by two, the second merged onto the first | S+I | `it::drafts::q9_if_match_concurrency` |
| Q10 | An approval names a `revision` or `digest`, or a send names a `revision`, that is no longer current | `409 draft_revision_mismatch` (`details.reason = "digest"` for a digest); no approval recorded, nothing sent, no event | S+I | `it::drafts::q9_if_match_concurrency`, `it::drafts::q19_approval_binds_content` |
| Q11 | `PATCH`, `DELETE` or reject while the draft is `sending` | `409 draft_not_editable` with `details.status = "sending"` and `retryable: true`; after the send it answers `409` (`sent`) | S | `it::drafts::q11_sending_not_editable` |
| Q12 | Two sends of one draft at once: an API send and its alarm fire, two API sends with different keys, two approvals with `send: true` (a person and a key) | Exactly one message; the other answers `409 draft_not_sendable` (`retryable: true`) or `200` with no second approval event, or the alarm skips it | S | `it::drafts::q12_concurrent_sends_one_message` |
| Q13 | `POST …/send` repeated with the same key (same body, another body, while in flight), a key reused on another draft, or a key first used for `POST …/messages` | Replay of the current draft with `deduplicated: true`; `409 idempotency_conflict` (with `details.original_draft_id` for another draft or operation); `409 request_in_progress` | S | `it::drafts::q13_send_idempotency` |
| Q14 | `POST …/send` with a new key on a `sent` draft | `409 draft_not_sendable` with `details.message_id`; no second message | S+I | `it::drafts::q13_send_idempotency` |
| Q15 | A key is minted at identity level with `drafts:approve` | `400 invalid_request` with `details.reason = "permission_not_allowed_for_level"` | S | `it::drafts::q15_identity_key_cannot_approve` |
| Q16 | An API key approves where only people may (`PM_QUARANTINE_KEY_RELEASE=off`, `drafts.key_approval` false), also on a tenant with only `quarantine.key_release: true` | `403 permission_denied` with `details.reason = "person_required"`; rejecting still works; with `drafts.key_approval: true` a platform key and the tenant's own partner key approve, and a tenant key still gets `403 permission_denied` (`person_required`); with `PM_QUARANTINE_KEY_RELEASE=on` any key with `drafts:approve` approves; a console owner approves after re-authentication | S | `it::drafts::q16_key_approval_rule` |
| Q17 | A tenant key with `policy:write`, or a console admin, sets `drafts.key_approval`; another partner's key sets it | `403 scope_denied` with `details.reason = "not_writable"`; the other partner's key gets `404 tenant_not_found`; the tenant's own partner key and a platform key set it | S | `it::drafts::q16_key_approval_rule` |
| Q18 | A tenant key loosens `drafts.require_approval` (`always` → `never`) where only people may decide; an identity key lowers its own `send_policy.require_approval`; a tenant at `never` with an identity override `new_recipients` | `403 permission_denied` (`person_required`, `details.field`); `403 scope_denied` (`details.field = "send_policy.require_approval"`); tightening is always accepted; the effective level is the stricter, so the identity needs approval for new recipients | S | `it::drafts::q18_require_approval_guard` |
| Q19 | The writer changes the content of an approved, scheduled or pending draft; or changes only `labels`, `metadata` or `send_at` | Content: the approval and send request are dropped, the draft is `pending_approval` (or `editing` when it never had an approval or request), `draft.approval_requested` with the new revision, and a scheduled draft leaves the schedule; labels, metadata or `send_at` only: the approval and schedule stay | S | `core::drafts::digest_canonical`, `it::drafts::q19_approval_binds_content` |
| Q20 | The key that wrote the current revision approves it | `403 permission_denied` with `details.reason = "self_approval"`; nothing recorded | S | `it::drafts::q20_approver_rules` |
| Q21 | Approve with `send: true` by a key without `messages:send` | `403 permission_denied` with `details.required = ["messages:send"]`; nothing recorded; the same key may approve with `send: false` | S | `it::drafts::q20_approver_rules` |
| Q22 | Approve or reject a draft that is `sent`, `canceled`, `sending`, `rejected`, or already decided the same way; two approvals of one revision race | `409 draft_not_pending` with `details.status` (reject while `sending`: `409 draft_not_editable`, retryable); a repeated decision answers `200` with no second event; of two racing approvals one records the approval | S | `core::drafts::state_table`, `it::drafts::q22_decision_states` |
| Q23 | Nobody decides a pending draft for 7 days (or until `send_at` + 1 hour, when later) | `rejected` with reason `expired` and `draft.rejected` (`reason: "expired"`), set by the `drafts` alarm purpose; a later approve gets `409 draft_not_pending` | S | `it::drafts::q23_pending_expiry_and_late_approval` |
| Q24 | `on_behalf_of` with `@`, over 128 characters, or with a byte outside printable ASCII; `on_behalf_of` sent by a console form | `400 invalid_request`; a valid value is stored in the approval, the audit row and `draft.approved`; the console records `actor_user_id` instead | S+I | `core::drafts::on_behalf_of_and_client_id`, `it::drafts::q24_on_behalf_of_audit` |
| Q25 | A reply draft's recipients resolve differently at send than at approval (the `Reply-To` became a known contact, D3) | The digest no longer matches: the draft returns to `pending_approval` with `draft.approval_requested`; nothing is sent to the new recipient without a new approval | S | `it::drafts::q25_reply_recipients_changed` |
| Q26 | Before a scheduled draft fires: the approving key is revoked, the approving person is removed or demoted, `drafts.key_approval` is turned off after a key approval; or the scheduling key loses `messages:send` while no approval is needed | Approval needed and no longer valid: back to `pending_approval` with `draft.approval_requested`, never sent; approval not needed and the authority invalid: `failed` with `authority_revoked` | S | `it::drafts::q26_approver_or_authority_revoked` |
| Q27 | An agent with `messages:send` sends directly (`POST …/messages`, reply, reply-all, forward, or a dry run) where the policy needs approval for the recipients | `403 approval_required` with `details.reason` and `details.recipients`; nothing stored, no hold, no idempotency record; the same request made as a draft and sent waits for approval; the system identity's sends are never refused; at `never` nothing changes | S+I | `it::drafts::q27_direct_send_approval_required` |
| Q28 | Recipient facts for the levels: an address of another identity of the same tenant; an address at the tenant's verified own domain; a subdomain of it; another workspace's address on the shared platform domain; a known external address; a `bcc` recipient | Internal, internal, external, external; known external needs approval at `external` and not at `new_recipients`; `bcc` counts like `to`; each level needs approval in a superset of the cases of the one below | S | `core::drafts::approval_levels`, `it::drafts::q27_direct_send_approval_required` |
| Q29 | An approved draft to an address the identity never wrote to, with `require_known_recipient: true`; the same address on the suppression list or a send-block entry | Unknown but approved: delivered (rule 4), and afterwards known by `outbound_count`; suppressed or blocked: the delivery is `suppressed` despite the approval | S | `it::drafts::q29_approved_recipients_known` |
| Q30 | A draft is sent now | `202`; the draft is `sent` with `message_id` in the same transaction as the message; its content is cleared; the message carries the draft's labels plus `labels_add` minus `labels_remove`, its metadata and `draft_id`; `draft.sent` | S | `it::drafts::q30_send_now_record_and_labels` |
| Q31 | `POST …/send` is refused by the pipeline (paused identity, `402`, daily cap, `thread_busy`, domain not ready) | The send's own error; the draft stays as it was; no idempotency record; after the condition clears the same key sends once | S+I | `it::drafts::q31_sync_refusal_leaves_draft` |
| Q32 | The Worker dies after the claim and before the accept transaction, for an API send, an approval send and a fire | No message exists. After 2 minutes: API origin back to its prior status; approval and alarm origins `scheduled` again, at most three attempts, then `failed` with `send_interrupted`; never two messages | S | `it::drafts::q32_claim_expiry_no_duplicate` |
| Q33 | The message made from a draft later ends `uncertain`, `failed`, `bounced` or `suppressed` | The draft stays `sent`; the message's own events report it; nothing resends it | S | `it::drafts::q30_send_now_record_and_labels` |
| Q34 | A send dry run of a draft that would send, schedule or wait for approval | `200` with `outcome` `send`, `schedule` or `pending_approval`, `would_send` false only for the last, and per-recipient status (rule 4 applied for `pending_approval`); nothing changes; the key is optional and unrecorded | S | `it::drafts::q34_dry_run` |
| Q35 | Many drafts scheduled on one identity: equal times, rescheduling one earlier, cancelling the earliest | One alarm at the earliest due time; drafts fire in `send_at`, `id` order, ten per run, then the alarm re-arms at once; rescheduling earlier re-arms earlier; cancelling the earliest re-arms to the next | S | `it::drafts::q35_alarm_multiplexing` |
| Q36 | The alarm handler runs twice for one fire, or concurrently with an API request on the same draft | At most one message (the claim and the internal key) | S | `it::drafts::q12_concurrent_sends_one_message` |
| Q37 | The mailbox's alarm is lost (its six retries exhausted, or never re-armed) while a draft is scheduled | The next request to the mailbox re-arms it; otherwise the every-minute watchdog finds the draft in `draft_index` 2 minutes past `send_at` and wakes the mailbox; `drafts_overdue` alerts at 10 minutes | S | `it::drafts::q37_missed_alarm_catch_up` |
| Q38 | A fire meets a transient condition (D1 down, deployment frozen or read-only, `thread_busy`, a timeout), and a fire that cannot run until after `send_at` + 1 hour | The draft stays `scheduled` and is tried again at 30 s, 1, 2 and then every 5 minutes; past the late limit it becomes `failed` with `send_window_missed` and the last code in `failure.detail`, and is never sent by itself | S | `it::drafts::q38_late_limit_and_transient` |
| Q39 | Every fire-time condition of [§6.3](#63-the-fire) (paused identity, suspended tenant or partner, sending paused, no owner, `402`, daily cap, retired explicit sender, domain verifying, failing domain, marketing on Cloudflare, test-mode recipient, loop, auto-reply refused, oversize, all recipients suppressed, approval now needed) | The outcome and `failure.reason` of the table, `draft.failed` for each failure; a failing domain sends through fallback; all suppressed is `sent` with `message_status: "suppressed"`; a paused identity's draft is not sent when it resumes | S | `it::drafts::q39_fire_outcomes` |
| Q40 | Reschedule (`PATCH send_at`), unschedule (`send_at: null`), retry a failed draft by `PATCH send_at`, and a drafts-only key changing the content of a draft another key scheduled | Reschedule keeps the approval; unschedule gives `approved` or `editing`; the failed draft is `scheduled` and every check runs again at the fire; the content change takes it off the schedule, so the other key's authority never sends content it did not see | S | `it::drafts::q40_reschedule_unschedule` |
| Q41 | An approval with `send: true` after `send_at` has passed | Sent at once; the console says so before the approver confirms | S | `it::drafts::q23_pending_expiry_and_late_approval` |
| Q42 | The identity is deleted, or the tenant erased, while drafts are scheduled; an alarm fires on the erased mailbox | The drafts are erased with the mailbox and their index rows deleted; the alarm does nothing and arms nothing; the watchdog skips erasing and erased tenants | S | `it::drafts::q42_erased_mailbox_alarm` |
| Q43 | A forward draft whose source was erased (a race), whose source has `body_truncated` and its raw MIME expired, or whose re-attached attachment object is missing; a source with a complete body and expired raw MIME | `409 source_unavailable` (`erased`, `raw_expired`, `attachment_missing`) on send or approve, `failed` with `source_unavailable` at a fire; the complete source forwards normally | S | `it::drafts::q43_source_at_send` |
| Q44 | A reply draft to a quarantined message is sent by a key without `quarantine:review` | `409 source_unavailable` (`not_visible`); with `quarantine:review` it sends | S | `it::drafts::q43_source_at_send` |
| Q45 | A draft names a `screening` message as its source | As for `quarantined`: only with `quarantine:review`; at send the authority needs it while the message is still `screening` | S | `it::drafts::q3_source_visibility` |
| Q46 | Message-scope erasure of a message that drafts reply to or forward, and of a message that was sent from a draft | Those drafts and the record of the sent one are deleted with their inline objects, outbox events and index rows; the receipt counts `drafts_deleted`; the probes find nothing | S | `it::drafts::q46_erasure_scopes` |
| Q47 | Thread, counterparty, identity and tenant erasure with drafts; a draft in a thread under a legal hold; a `new` draft without a thread | Every draft of the scope is deleted and counted (counterparty: the address in `to`, `cc`, `bcc` or the resolved reply recipients); a held thread's drafts are kept and the thread listed in `receipt.held`; a threadless draft has no hold | S | `it::drafts::q46_erasure_scopes` |
| Q48 | Retention: an `editing` draft untouched for 30 days, a `rejected` one, a `sent` record, a `scheduled` draft 60 days ahead, a tenant with `retention.message_days: 7`, a draft in a held thread | Deleted after 30 days (7 with the tenant's setting) with `drafts_deleted` in the audit row; the scheduled draft is kept until its fire; the held one is kept | S | `it::drafts::q48_retention` |
| Q49 | A scheduled send meets `402 billing_limit`; the workspace upgrades and the writer reschedules it | `failed` with `billing_limit`, no hold kept, no idempotency record; the explicit reschedule sends once with the same internal key | S | `it::drafts::q49_billing_at_send` |
| Q50 | Several scheduled sends fail with `402` in one period | One `billing.limit_reached` for the period; the usage alerts are those already sent at 80% and 100%; a `draft.failed` per draft | S | `it::drafts::q49_billing_at_send` |
| Q51 | An endpoint subscribes to `draft.*` with only `webhooks:manage`; a draft event's payload | Allowed (no mail content, J29 does not apply); `data` holds no subject, body, address, filename, note or reason text; the event has the identity's `sequence` and the same `evt_` ID in the event feed | S | `it::drafts::q51_events_thin` |
| Q52 | A platform key calls a draft route on the system identity | `404 identity_not_found` | S | `it::drafts::q52_system_identity_no_drafts` |
| Q53 | Another tenant's key, another partner's key, or an identity key of another identity reaches a draft; an identity key reads the tenant queue | `404 draft_not_found`, the same body as a missing draft; `403 scope_denied` on the queue | S | `it::drafts::q53_scope_matrix`, `it::security::cross_tenant_matrix` |
| Q54 | A draft's text, subject, filenames, note or `on_behalf_of` contain HTML, scripts or confusable characters | The console shows them escaped, the HTML only in the token-less sandboxed frame, and loads no remote content | S | `it::console::drafts_page` |
| Q55 | A console approve without a sign-in in the last 10 minutes; a viewer or member posts approve; the page with JavaScript off | `303` to `/console/reauth`; `403`; every form works and the page renders without JavaScript | S | `it::console::drafts_page`, `browser::console::no_js` |
| Q56 | A test tenant's draft is sent to a simulator address, to another identity of its tenant, and to an outside address | Simulator and loopback as for any test send; outside: `403 test_mode_recipient` (API) or `failed` (fire) | S | `it::drafts::q56_test_tenant` |
| Q57 | Drafts on a suspended tenant and on a paused identity | Suspended: every draft write `403 tenant_suspended`, reads work. Paused: create, edit, approve without sending and reject work; send and approve with `send: true` refuse or fail with `identity_paused` | S | `it::drafts::q57_suspended_and_paused` |
| Q58 | D1 fails while drafts change; the tenant queue is read meanwhile; an approve arrives for a draft the stale index still shows pending | The mailbox commits; the index rows wait and are written in order with back-off; the queue may show an older state; the approve answers from the mailbox (`409 draft_not_pending` or `200`); a stuck row alerts after 15 minutes | S | `it::drafts::q58_index_lag` |
| Q59 | What `draft_index` holds | IDs, status, kind, counts, times, key IDs and codes only; no subject, address, body, filename or note | S | `it::drafts::q58_index_lag` |
| Q60 | MCP: a key without `drafts:write` lists tools; `mail_send_draft` without `idempotency_key`; any approve tool | Only the tools its permissions allow; `idempotency_key_required`; no approve or reject tool exists | S | `it::mcp::drafts_tools` |
| Q61 | A revision saved with an upload reference, then the upload is deleted or expires before the draft is sent or its scheduled send fires; a stored copy is missing at send | The revision's own copy is sent and the attachment's `upload_id` records the provenance; a missing or changed copy (a storage fault) fails the send with `source_unavailable` (`attachment_missing`) | S | `it::drafts::q6_upload_references` |
| Q62 | `DELETE` of a scheduled draft, a second `DELETE`, and `DELETE` of a `sent` draft | `canceled`, content and inline objects deleted, never sent; `200` again with no change; `409 draft_not_editable` | S | `it::drafts::q62_cancel` |
| Q63 | A rejected draft: send it, approve it, request approval again without a change, then edit it | `409 draft_not_sendable`, `409 draft_not_pending`, `409 draft_not_sendable`; the edit makes a new revision in `editing` (or `pending_approval` with `request_approval`) | S | `it::drafts::q22_decision_states` |
| Q64 | Message-scope erasure leaves a thread empty while a `new` draft continues that thread | The thread is deleted and the draft with it, counted in `drafts_deleted` | S | `it::drafts::q46_erasure_scopes` |

## 22. Tests

| Test | Proves | Covers |
|---|---|---|
| `core::drafts::kind_field_rules` | The field table of §2.2 for each kind, at create and patch; `in_reply_to` with `forward_of`, `reply_all` alone and kind fields in a patch are refused with their paths | Q2, Q4 |
| `core::drafts::state_table` | Every row of §2.3 (T1–T17) as a table test over status × action × actor: the resulting status, event and error, including the refusals for `sending`, `sent`, `canceled` and `rejected` | §2.3, Q22, Q63 |
| `core::drafts::approval_levels` | `approval_needed` for each level over generated recipient facts, and the property that each level needs approval in a superset of the cases of the one below; the `internal` rule (same-tenant identity, verified own domain, no subdomain, platform domain only by identity) | Q28 |
| `core::drafts::digest_canonical` | Key order, address case and list order do not change the digest; `send_at`, labels and metadata do not; any change of a digest field, display names included, does; reply drafts use the resolved recipients | Q19, Q25 |
| `core::drafts::etag_and_send_at` | The etag format and strong comparison; `send_at` bounds at 59.999 s, 60 s, 90 d and 90 d + 1 ms; offsets converted; past and unparseable values refused | Q7 |
| `core::drafts::on_behalf_of_and_client_id` | Lengths, printable ASCII and the `@` refusal for `on_behalf_of` and `client_id` | Q1, Q24 |
| `it::drafts::q1_client_id_idempotent_create` | Q1, including the race | Q1 |
| `it::drafts::q3_source_visibility` | Q3 and Q45 for create, update, list and get | Q3, Q45 |
| `it::drafts::q5_oversize_and_bounds` | Q5 and the `send_at` errors of Q7 at create, update and send; no R2 object left after a refused create | Q5, Q7 |
| `it::drafts::q6_upload_references` | Q6 and Q61, with uploads at each state; the copy at save; the upload deleted and expired after save; the SHA-256 check at send | Q6, Q61 |
| `it::drafts::q8_draft_limit` | The 1,001st draft and the race for the last place | Q8 |
| `it::drafts::q9_if_match_concurrency` | Q9, and the stale-revision approve and send of Q10 | Q9, Q10 |
| `it::drafts::q11_sending_not_editable` | `PATCH`, `DELETE` and reject during a send held open by a fault plan on `r2.get` with a delay | Q11 |
| `it::drafts::q12_concurrent_sends_one_message` | The races of Q12 and a doubled alarm (`/__test/alarm` twice) of Q36 produce one message row, one idempotency row and one `draft.sent` | Q12, Q36 |
| `it::drafts::q13_send_idempotency` | Q13 and Q14 | Q13, Q14 |
| `it::drafts::q15_identity_key_cannot_approve` | Q15 | Q15 |
| `it::drafts::q16_key_approval_rule` | Q16 and Q17 across `PM_QUARANTINE_KEY_RELEASE`, `PM_CONSOLE`, `drafts.key_approval` and `quarantine.key_release` | Q16, Q17 |
| `it::drafts::q18_require_approval_guard` | Q18 for the tenant field and the identity override; `GET …/policy` reports `person_required_to_loosen` for the field | Q18 |
| `it::drafts::q19_approval_binds_content` | Q19 for each status, and the digest approve of Q10 | Q10, Q19 |
| `it::drafts::q20_approver_rules` | Q20 and Q21 | Q20, Q21 |
| `it::drafts::q22_decision_states` | Q22 and Q63 through the API | Q22, Q63 |
| `it::drafts::q23_pending_expiry_and_late_approval` | Q23 (with the fake clock and `/__test/alarm`) and Q41 | Q23, Q41 |
| `it::drafts::q24_on_behalf_of_audit` | The audit row `draft.approve` (written by the outbox dispatch, once even when the dispatch repeats) and `draft.approved` carry `on_behalf_of`; a console approval carries `actor_user_id` | Q24, §4.6 |
| `it::drafts::q25_reply_recipients_changed` | Q25 | Q25 |
| `it::drafts::q26_approver_or_authority_revoked` | Q26 | Q26 |
| `it::drafts::q27_direct_send_approval_required` | Q27 and the recipient facts of Q28 through real sends and dry runs | Q27, Q28 |
| `it::drafts::q29_approved_recipients_known` | Q29 | Q29 |
| `it::drafts::q30_send_now_record_and_labels` | Q30, and Q33 with simulator `timeout@` and `bounce@` recipients | Q30, Q33 |
| `it::drafts::q31_sync_refusal_leaves_draft` | Q31 for each refusal named | Q31 |
| `it::drafts::q32_claim_expiry_no_duplicate` | Q32 with `restart_runtime()` between the claim and the accept transaction ([Testing § 6.6](testing.md#66-fault-injection)) | Q32 |
| `it::drafts::q34_dry_run` | Q34 | Q34 |
| `it::drafts::q35_alarm_multiplexing` | Q35: 25 drafts across three times; the recorded alarm times and the fire order | Q35 |
| `it::drafts::q37_missed_alarm_catch_up` | Q37: the alarm deleted by a test hook; the watchdog cron (`/cdn-cgi/local/scheduled`) wakes the mailbox; `drafts_alarm_missed_total` increments | Q37 |
| `it::drafts::q38_late_limit_and_transient` | Q38 with `d1.query` faults and the fake clock: the recorded attempt times, then `send_window_missed` | Q38 |
| `it::drafts::q39_fire_outcomes` | One case per row of the §6.3 table | Q39 |
| `it::drafts::q40_reschedule_unschedule` | Q40 | Q40 |
| `it::drafts::q42_erased_mailbox_alarm` | Q42 | Q42 |
| `it::drafts::q43_source_at_send` | Q43 and Q44 | Q43, Q44 |
| `it::drafts::q46_erasure_scopes` | Q46, Q47 and Q64: receipts, probes, R2 prefixes and `draft_index` after each scope; holds | Q46, Q47, Q64 |
| `it::drafts::q48_retention` | Q48 | Q48 |
| `it::drafts::q49_billing_at_send` | Q49 and Q50 with a metered workspace at its `sends` allowance | Q49, Q50 |
| `it::drafts::q51_events_thin` | Q51; every draft event against its golden file | Q51, §15 |
| `it::drafts::q52_system_identity_no_drafts` | Q52 | Q52 |
| `it::drafts::q53_scope_matrix` | Q53; the nine routes join `it::security::cross_tenant_matrix` | Q53, NFR-SEC-1 |
| `it::drafts::q56_test_tenant` | Q56 | Q56 |
| `it::drafts::q57_suspended_and_paused` | Q57 | Q57 |
| `it::drafts::q58_index_lag` | Q58 with `d1.query` faults on `INSERT INTO draft_index`; Q59 by reading the D1 schema and every written row | Q58, Q59 |
| `it::drafts::q62_cancel` | Q62 | Q62 |
| `it::console::drafts_page` | The three screens for each role; approve needs re-authentication and posts `revision` and `digest`; a stale page is refused and shown again; the Overview items; escaping (Q54) | Q54, Q55, §16 |
| `browser::console::no_js`, `browser::console::axe_scan` | The drafts screens join both | Q55 |
| `it::mcp::drafts_tools` | Q60; each tool maps to its route and returns the same errors | Q60, §17.1 |
| `cli::drafts::send_key_derivation` | `pmail drafts send` derives `cli:{draft_id}:{revision}` and re-running it replays | §17.2 |
| `sdk::coverage::every_operation` | Still passes with the nine new operations | FR-SDK-1 |
| `live::drafts::scheduled_send` | On staging, a draft scheduled 2 minutes ahead to a real mailbox arrives once, within a minute of `send_at`, with its labels on the stored message | FR-DRF-7 |

## 23. Milestone

To be added to [Build plan](../build-plan.md) after M28:

> ## M29 · Drafts, approvals and scheduled send (after M8, M9, M14, M15, M16, M21, M22, M24 and M26)
>
> **Files:** `crates/core/src/drafts.rs` (the field table per kind, the state table, `approval_needed`,
> `digest`, the etag, the `send_at` window, the patch merge; pure), `crates/api-types/src/drafts.rs`,
> `crates/worker/src/handlers/drafts.rs` (the nine routes), `crates/worker/src/mailbox/drafts.rs`
> (storage, transitions, `SendDraft`, the `drafts` and `draft_index` alarm purposes),
> `crates/worker/src/drafts/attachments.rs`, `crates/worker/src/send/{context.rs, authority.rs}`,
> `crates/worker/src/crons/drafts_due.rs`, `crates/worker/src/console/pages/drafts.rs`,
> `crates/cli/src/commands/drafts.rs`. No migration while nothing is built: `draft_index` goes into
> `0001_init.sql`, and `drafts`, `draft_sync`, `messages.draft_id` and `idempotency.draft_id` into mailbox
> schema v1 ([Drafts §9](drafts.md#9-storage)). Hooks in other milestones' files, each reviewed by
> that file's owner:
>
> | File | Hook |
> |---|---|
> | `mailbox/submit.rs` (M9) | `SubmitInput.draft` and `SubmitInput.approved`; the draft arm of the accept transaction; policy step 16a |
> | `handlers/send.rs` (M9) | The D1 context loader moves to `send/context.rs`, unchanged, and gains the step-16a facts |
> | `crates/core/src/policy.rs` (M2, M9) | Known-recipient rule 4; step 16a; `FIELDS` gains `drafts.require_approval` (guard) and `drafts.key_approval` (platform or own partner) |
> | `policy/write.rs` (M5), `handlers/identities.rs` (M6) | The guard rule for `drafts.require_approval`; the stricter-only rule for `send_policy.require_approval` |
> | `auth/people.rs` (M21) | `key_may_decide` gains `Decision`, with `ApproveDraft` reading `drafts.key_approval` |
> | `webhooks/payloads.rs` (M8), `mailbox/outbox.rs` (M6) | `draft_summary`, `draft_event`; the audit rows of `draft.approved` and `draft.rejected` in the dispatch batch |
> | `jobs/erasure.rs`, `jobs/retention.rs` (M14) | The draft rows of every scope, `drafts_deleted` in the receipt; draft purge and pending expiry in `Hygiene` |
> | `mcp/tools.rs` (M15) | The six draft tools, with their rows in M15's table test; one sentence in the three send tools' descriptions |
> | `crates/cli/src/commands/drafts.rs` (M16) | `pmail drafts …` |
> | `console/pages/overview.rs` (M24) and the Notifier's needs-a-person counts (M26) | The two "Needs a person" items |
> | The every-minute cron (M5, M17 Foundation) | `crons/drafts_due.rs` |
> | `drafts/attachments.rs`, upload arm | Built by whichever of M29 and M33 (uploads) lands second, with `it::drafts::q6_upload_references` |
> | The draft field table (`core::drafts`) | M32 (calendar invitations) adds the `calendar` field to `new` and `reply` drafts if it lands after M29 |
>
> **Implements:** FR-DRF-1 … FR-DRF-14, the changes to FR-OUT-3, FR-OUT-13, FR-CON-5, FR-CON-12, FR-CON-15
> and FR-PRV-3, and edge rows Q1–Q64 ([Drafts](drafts.md)); the cross-tenant suite's new routes
> (NFR-SEC-1).
>
> **Acceptance:**
>
> - Edge rows Q1–Q64 with the tests named in the register, among them
>   `it::drafts::q12_concurrent_sends_one_message` and `it::drafts::q32_claim_expiry_no_duplicate` (one
>   message per revision), `it::drafts::q19_approval_binds_content`, `it::drafts::q27_direct_send_approval_required`,
>   `it::drafts::q35_alarm_multiplexing`, `it::drafts::q37_missed_alarm_catch_up`,
>   `it::drafts::q38_late_limit_and_transient` and `it::drafts::q39_fire_outcomes`.
> - `core::drafts::{kind_field_rules, state_table, approval_levels, digest_canonical, etag_and_send_at,
>   on_behalf_of_and_client_id}`.
> - Erasure and retention: `it::drafts::q46_erasure_scopes` and `it::drafts::q48_retention` fill M14's
>   rows.
> - `it::console::drafts_page`; the pages join `browser::console::no_js` and `browser::console::axe_scan`.
> - `it::mcp::drafts_tools`, `cli::drafts::send_key_derivation`; `sdk::coverage::every_operation` still
>   passes with the nine new operations; the routes join `it::security::cross_tenant_matrix`.
> - With `drafts.require_approval: never` and no draft, the M9 send suites (`it::send::*`) pass unchanged.
> - At M20: `live::drafts::scheduled_send`.

The dependency graph gains:

```text
M8, M9, M14, M15, M16, M21, M22, M24, M26 ─▶ M29 drafts, approvals and scheduled send ─▶ M20
   (M29 also adds six tools to M15's table and its commands to M16's CLI; the upload arm of
    drafts/attachments.rs lands with whichever of M29 and M33 comes second)
```

## 24. Compared with AgentMail (read 2026-10-17)

From the inventories of AgentMail's public docs and OpenAPI document read on 2026-10-17 (untrusted data).

| AgentMail | Pylota Mail | |
|---|---|---|
| Drafts with CRUD at inbox scope; read at org and pod scope | CRUD per identity; the tenant's queue of pending, scheduled and failed drafts from a content-free index | Match; different for the org-wide list (content stays in the mailbox, [§1](#1-why-the-mailbox-and-a-content-free-index-in-d1)) |
| `send_at` schedules a draft (label `scheduled`, `send_status`) | `status: scheduled`, driven by the mailbox alarm, with a 60 s–90 day window | Match; exceed: a watchdog for missed alarms and a defined late limit |
| Reschedule by updating `send_at`; unschedule with `send_at: null`; cancel by deleting | The same three | Match |
| A failed scheduled send (`send_status: failed`) is retried by updating `send_at` | The same, as an explicit act; `failure.reason` names one of the defined outcomes of §6.3 | Match; exceed |
| `in_reply_to` derives subject, threading and recipients; `reply_all` cannot be combined with `to`, `cc` or `bcc`; `in_reply_to` and `forward_of` are exclusive; the kind is fixed at creation | The same rules, with recipients resolved at send time and `From` chosen by FR-OUT-5 | Match |
| `forward_of` merges the original's body and attachments at send time | Snapshot at send time; `409 source_unavailable` when the source cannot be reproduced in full | Match; exceed |
| Drafts cannot read an expired source; a scheduled send needing one fails and is not replayed | `source_unavailable`, only when the source really cannot be reproduced | Match; exceed (no failure for a complete source) |
| Labels carry over to the sent message; `drafts.send` can add and remove labels | `labels_add` and `labels_remove` on send | Match |
| `client_id` makes draft creation idempotent | The same, plus `409 client_id_conflict` for a different body | Exceed |
| `Idempotency-Key` on `drafts.send`, kept 24 hours | Required, kept 30 days; the draft revision's own key `draft:{draft_id}:{revision}` inside | Exceed |
| `409` while a draft is being sent | `409 draft_not_editable` / `draft_not_sendable` with `retryable: true` | Match |
| No concurrency control on update | `revision`, `etag`, `If-Match` | Exceed |
| The draft is deleted once sent; a second send without the key answers `404` | Kept 30 days as a record with `message_id`; a second send answers `409` with `message_id`; the message keeps `draft_id` | Deliberately different: the approval trail survives the send |
| A draft is an approval step only when the agent's key lacks `message_send`, `message_reply` and `draft_send` | Two patterns: withheld `messages:send`, or a policy that needs approval; approval bound to the content digest; person-only on Cloud unless the partner opts in; no self-approval; `on_behalf_of` audited | Exceed |
| Permissions `draft_read`, `draft_create`, `draft_update`, `draft_delete`, `draft_send` | `drafts:read`, `drafts:write`, `drafts:approve`, and `messages:send` to send | Deliberately different: approval is its own permission, and create, update and delete are one right |
| A paused inbox fails its scheduled drafts, which are not retried on resume | The same (`identity_paused`) | Match |
| The send allow-list applies to draft sends | Every send check applies, plus approval rules | Match; exceed |
| `reply_to` field on drafts | Not offered | Deliberately different: `Reply-To` carries the thread token ([Outbound › Reply-To](outbound.md#reply-to-and-the-thread-token)); a free `Reply-To` would break threading and redirects answers, a phishing aid |
| Get a draft attachment | `GET …/drafts/{draft_id}/attachments/{attachment_id}` | Match |
| No documented bound on `send_at` | 60 seconds to 90 days | Exceed (defined behaviour) |
| No documented events for drafts | Five thin `draft.*` events with the identity's sequence | Exceed |

## 25. Integration

The shared pages carry this page's contracts: the PRD, the REST API reference and `openapi.yaml`, the
errors, events, limits and configuration references, the data model, security, workspace policy and the
other design pages it extends, the MCP and CLI references, the edge-case register and the build plan
([ADR 0017 § Integration](../adr/0017-agentmail-parity.md#integration), integrated on 2026-10-18). A
difference between this page and one of them is a defect: fix both in the same change.
