# Uploads

Binding for implementation. An agent can stage a file once, as an **upload** that belongs to one identity,
and then attach it to sends, replies, reply-alls, forwards and drafts by its ID instead of carrying the
bytes as base64 inside every JSON request. Today every attachment travels as `content_base64` inside a
request body of at most 7 MiB, which inflates it by a third, and an MCP client must make the model emit
those bytes as text. An upload is sent to the Worker once, verified for size and SHA-256 while it streams
into R2, classified with the same `core::attach` rules as inbound mail, and then copied into every message
that uses it. Uploads are not a drive: they have no paths, no folders, no download route and a lifetime of
at most 7 days. Pylota Mail never fetches an attachment from a URL ([§1](#1-what-uploads-are-and-are-not)).

| | |
|---|---|
| Requirements | FR-UPL-1 to FR-UPL-12 (this page), FR-OUT-10, FR-IN-5, FR-BILL-8, FR-PRV-1, FR-PRV-3 |
| Edge cases | [V1](../edge-cases.md)–[V44](../edge-cases.md) |
| Code | `crates/core/src/uploads.rs` (create validation, the state machine, the upload-token payload, expiry arithmetic; pure), `crates/core/src/attach.rs` (`classify_name`, used again with the final filename), `crates/core/src/compose.rs` (`encoded_size`, the link plan from sizes alone), `crates/worker/src/uploads/{mod.rs, put.rs, classify.rs, copy.rs}`, `handlers/uploads.rs` (four routes), `handlers/links.rs` (`PUT /v1/links/{token}`), `mailbox/uploads.rs` (the `uploads` table, its `MailboxRequest` operations and `alarm:uploads`), the attachment hooks in `mailbox/submit.rs` (policy step 12, steps 6 and 7 of the reservation), the `uploads` rows in `jobs/erasure.rs`, the `up/` exclusion in `jobs/backup.rs`, `mcp/tools.rs`, `crates/cli/src/commands/uploads.rs`; the R2 additions to `BlobStore` in `crates/platform/src/lib.rs` and `cf/glue.rs` |
| Tables | Mailbox `uploads` (new), `attachments.upload_id` (new column), `meta` key `alarm:uploads`; D1: none ([Data model](data-model.md)) |
| Contracts | `POST`, `GET` (list and one) and `DELETE` on `/v1/identities/{identity_id}/uploads…`, and `PUT /v1/links/{token}` ([REST API](../../reference/api.md)); `attachments[].upload_id` on send, reply, reply-all and forward, and `attachments` on forward; `Message.attachments[].upload_id`; the permissions `messages:send` or `drafts:write` (either one); the signed-link purpose `upload`; the receipt field `uploads_deleted`; the errors `404 upload_not_found`, `409 upload_not_ready`, `409 upload_in_progress`, `409 upload_in_use`, `410 upload_expired`, `400 upload_size_mismatch`, `400 upload_checksum_mismatch`, `422 upload_limit_reached` and `422 attachment_risky` ([Errors](../../reference/errors.md)); the MCP tools `mail_create_upload` and `mail_get_upload`; no new event and no new policy field |
| Limits | [Limits › Uploads](../../reference/limits.md#uploads): 1–26,214,400 bytes per upload; 100 live uploads and 524,288,000 declared bytes per identity; lifetime 900–604,800 seconds, default 86,400; upload URL 3,600 seconds; attachments per message 26,214,400 decoded bytes in total |
| Built in | [M33](../build-plan.md#m33--uploads-after-m7-m9-m14-m15-m16-and-m22) |

## 1. What uploads are and are not

**The problem.** A send carries each attachment as `content_base64` ([Outbound › Attachments and size](outbound.md#attachments-and-size)).
The request body is at most 7 MiB (`413 payload_too_large`, [Outbound › Handler](outbound.md#handler) step 4),
so the largest file a send can carry is about 5.25 MiB, and with `large_attachments: "link"` the signed
link that replaces an oversized file can never be bigger than that either. An MCP client is worse off:
`tools/call` carries its `arguments` as a JSON object, and the protocol has no binary input type, so a
file reaches a tool only as text the model writes out
([MCP specification 2026-07-28, Tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools),
read 2026-10-17). A 3 MiB PDF is four million base64 characters of model output.

**What an upload is.** A file staged by one identity: declared up front (filename, content type, size,
SHA-256), sent once to a signed URL on the Worker, verified, classified, and then attachable by
`{ "upload_id": "upl_…" }` until it expires. Every message that uses it gets its own copy of the bytes,
so expiring or deleting the upload never changes a sent message ([§5.4](#54-reuse-copy-never-reference)).

**Pylota Mail never fetches attachments from URLs.** AgentMail accepts `url` attachments and fetches them
at send time. Pylota Mail does not, for three reasons:

- **SSRF.** A fetch on behalf of a request would be an outbound request to an address chosen by the caller.
  Every outbound destination Pylota Mail calls today is fixed by the operator and passes the SSRF guard of
  [Security § 9](security.md#93-other-outbound-destinations); a per-request URL would turn every send key
  into a way to probe internal or link-local addresses, follow redirects into them, or make the Worker
  download from a host that serves something different to Cloudflare's addresses.
- **Content integrity.** The bytes at a URL can change between the moment an agent or a person decides
  to send them and the moment the Worker fetches them, and again on a retry. An upload's SHA-256 is
  declared before the bytes arrive and checked by R2; a replayed send can only ever attach the same
  bytes.
- **Prompt injection.** Agents read untrusted mail. "Attach the file at https://…" in a message is an
  instruction an agent should not follow; with no URL attachments, following it cannot make Pylota Mail
  fetch and forward anything.

**Not a drive.** AgentMail's Agent Drive keeps files under paths with listing, download links and
per-file expiry. Uploads have none of that: no path, no folder, no rename, no download route (the bytes
are only ever read back by the Worker to classify and copy them), no sharing between identities, and a
lifetime of at most 7 days. Long-lived files belong to the integrator's own storage (for Pylota, its own
object store); an upload is the staging step of a send.

**Not a presigned R2 URL.** R2 presigned URLs need an R2 API token's Access Key ID and Secret Access Key
and S3 SigV4 signing, are bearer URLs valid for 1 second to 7 days, and work only on the S3 API domain
`<ACCOUNT_ID>.r2.cloudflarestorage.com`
([R2 presigned URLs](https://developers.cloudflare.com/r2/api/s3/presigned-urls/), updated 2026-08-22,
read 2026-10-17). Using them would add a long-lived S3 credential that can sign for the whole bucket,
expose the account ID in every upload URL, and let bytes land in R2 with no Worker in the path: no check
of a tenant suspension or identity pause at upload time, no attempt counting, and no classification until
a separate "complete" call. A Worker URL keeps the one credential the deployment already has (the `link`
signing key, [Security § 7.3](security.md#73-signed-links)) and does every check in the request that
carries the bytes.

**Why the Worker can take the bytes.** A Worker accepts request bodies of up to 100 MB on the Free and Pro
account plans, 200 MB on Business and up to 5 GB on Enterprise, and answers `413` above that
([Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17). The
upload maximum of **26,214,400 bytes (25 MiB, a Pylota Mail choice)** is below every plan's limit, so it
is a fixed number rather than a setting. It is also the largest message Cloudflare Email Routing delivers
inbound ([Limits › Mail](../../reference/limits.md#mail)), so an agent can forward or re-send anything it
can receive, and it bounds the memory classification needs ([§4.3](#43-classification-and-the-scanner)).
On Workers Paid an HTTP request has no wall-time limit while the client stays connected
([Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17), so a slow
client is not cut off; the upload URL's one-hour lifetime is checked when the request starts, not while
it streams.

## 2. The upload

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
| `filename` | Sanitised as attachment filenames are (no path components, no control characters, at most 255 bytes, [Inbound › Risk classification](inbound.md#risk-classification)). Untrusted content |
| `content_type` | As declared: a valid `type/subtype`, at most 127 bytes |
| `size` | Declared bytes, 1–26,214,400. The bytes must match it exactly |
| `sha256` | Declared SHA-256 of the bytes, 64 hexadecimal characters; stored and returned lower case |
| `status` | `pending`, `uploading`, `ready`, `rejected` or `expired` (below) |
| `status_reason` | For `rejected`: `risk` or `infected`. For `expired`: `expired`, `too_many_attempts` or `object_missing`. Otherwise `null` |
| `sniffed_type`, `risk` | Set when the bytes are classified ([Inbound › Sniffing](inbound.md#sniffing)); `risk` is one of the six `AttachmentRisk` values or `null` |
| `scan_status` | `skipped` (no scanner configured), `clean`, `infected` or `error` |
| `use_count`, `last_used_at` | Accepted messages that copied this upload, and when the last one did |
| `expires_at` | When the upload and its bytes are deleted, whether or not it was used |
| `upload_url`, `upload_url_expires_at` | Present only in the create response and in `GET` of one upload while it is `pending` or `uploading`; `null` otherwise and never in lists. A new URL is minted on every such response ([§3.2](#32-sending-the-bytes--put-v1linkstoken)) |

```text
                  PUT claims             bytes verified, classified clean
 create ──▶ pending ─────────▶ uploading ─────────────────────────────▶ ready ──┐
               ▲                  │  │                                          │
               └── PUT failed ────┘  └── quarantining risk or infected ──▶ rejected
                   (or lease lapsed;         (object deleted)                     │
                    10th failure ──▶ expired)                                     │
 pending, uploading, ready ── expires_at passes ──────────────────────────▶ expired
 rejected, expired ── 7 days later ─▶ row deleted (then 404 upload_not_found)
 DELETE (any state, unless a send holds it) ─▶ row and object deleted
```

- **`pending`.** Created; no verified bytes. Only a `PUT` with this upload's token moves it on.
- **`uploading`.** One `PUT` holds the claim (`put_token`, a lease of 120 seconds that the `PUT` renews
  every 60 seconds). A crashed `PUT` loses its claim when the lease lapses; the next `PUT` takes it.
- **`ready`.** The bytes in R2 have the declared size and SHA-256, were classified, and carry no
  quarantining risk (`encrypted_document` is the one non-quarantining risk, and it is allowed). Only
  `ready` uploads can be attached.
- **`rejected`.** The bytes carry a quarantining risk (`executable`, `macro`, `archive_bomb`,
  `encrypted_archive`, `type_mismatch`) or the scanner said `infected`. The object is deleted at once; the
  row stays as a tombstone so that a later attach gets `422 attachment_risky` instead of `404`.
- **`expired`.** `expires_at` passed, ten `PUT` attempts failed, or the object was found missing (after a
  restore). The object is deleted; the row stays as a tombstone for 7 days so that a late attach gets
  `410 upload_expired`.

## 3. Routes

All four `…/uploads` routes are identity-scoped (`Scope::IdentityPath`) and need **either**
`messages:send` **or** `drafts:write`: the key must hold at least one. They are available to every key
level within its scope (an identity key for its own identity). Rate limits: `RL_API` and, for tenant and
identity keys, `RL_TENANT`, as for every route.

### 3.1 Create — `POST /v1/identities/{identity_id}/uploads`

```json
{
  "filename": "FNOL-7781.pdf",
  "content_type": "application/pdf",
  "size": 9437184,
  "sha256": "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
  "expires_in": 86400
}
```

`201` with the upload object (status `pending`, with `upload_url`). `Idempotency-Key` is optional; a replay
returns the original status (`201`) with `Idempotent-Replayed: true`, and, like every replay that carries
a resource, it **re-reads the upload** and mints a fresh `upload_url` (the stored record keeps the upload
ID, never a URL). In order:

1. Authentication, permission (`messages:send` or `drafts:write`), level and owner checks
   ([Security § 5.2](security.md#52-order-of-checks)); a missing or foreign identity is
   `404 identity_not_found`.
2. Body: `filename` 1–255 bytes after sanitising (empty after sanitising → `400 invalid_request`),
   `content_type` a valid `type/subtype` of at most 127 bytes, `size` an integer 1–26,214,400,
   `sha256` matching `^[0-9A-Fa-f]{64}$`, `expires_in` an integer 900–604,800 (default 86,400). Each
   failure is `400 invalid_request` with `details.errors[]`.
3. D1 facts (one batch, the same reads as the send handler's step 6 for identity and tenant): identity
   `deleting` or `deleted` → `404 identity_not_found`; tenant `suspended` → `403 tenant_suspended`;
   identity `paused` → `409 identity_paused` with `details.reason`; the tenant's effective
   `retention.message_days` and the identity's `retention.message_hours`
   ([Parity additions](parity-additions.md)), for the expiry cap in step 6.
4. Storage gate: `QuotaRequest::GetUsage` on the tenant's `TenantQuota`; when `storage_gb` has a
   `granted` value and `used ≥ granted` → `402 billing_limit` with `details.feature = "storage_gb"`
   ([Plans, metering and billing › Storage](billing.md#storage)). Skipped for `billing` `exempt` or
   `disabled`.
5. `MailboxRequest::CreateUpload` ([§5.2](#52-mailbox-operations)) checks, inside one transaction, that the
   identity has fewer than 100 live uploads (`pending`, `uploading`, `ready`) and that their declared
   sizes plus this one are at most 524,288,000 bytes; otherwise `422 upload_limit_reached` with
   `details.limit` (`count` or `bytes`), `details.used` and `details.max`. It inserts the row.
6. `expires_at = created_at + expires_in`, capped at `created_at + retention.message_days days` and at
   `created_at + retention.message_hours hours` when those are set. The response always shows the
   effective value.
7. Mint the upload URL ([§3.2](#32-sending-the-bytes--put-v1linkstoken)) and return `201`.

### 3.2 Sending the bytes — `PUT /v1/links/{token}`

```http
PUT /v1/links/bDE6YTp1cGxvYWQ6dGVuXzAxSjk… HTTP/1.1
Host: mail.example.com
Content-Type: application/octet-stream
Content-Length: 9437184

<9,437,184 bytes>
```

`200` with the upload object (status `ready`, `upload_url: null`). The URL needs no API key: it is a signed
link in the format of [Security § 7.3](security.md#73-signed-links) with a new purpose:

```text
payload = "l1:{kid}:upload:{tenant_id}:{identity_id}:{upload_id}:{expires_unix_s}"
token   = base64url(payload || HMAC-SHA256(link key {kid}, payload)[0..16])
expires = min(now + 3,600 s, upload.expires_at)
```

The token is a capability for exactly one upload, whose size and SHA-256 were fixed at create; it cannot
put different bytes. It is a bearer URL, so it is short-lived (one hour) and a new one is minted on every
create response and every `GET` of a `pending` or `uploading` upload. An agent can hand the URL to a tool
that holds the file but no API key (a code sandbox, a shell, a browser automation step).

Purposes are separate: a `PUT` with an `att` or `export` token, or a `GET` with an `upload` token, gets
`404` with the code of the token's own target (`upload_not_found` for an upload token, as for a bad MAC),
so a link never reveals why it failed. `/v1/links/*` answers only on the API host, sends no CORS headers
and is never called by the console.

Rules in order. "Count a failure" means `MailboxRequest::FailUpload { counted: true }`, which returns the
upload to `pending` and increments `failed_puts`; the tenth counted failure sets `expired`
(`too_many_attempts`) and deletes the object:

1. **Token.** Split, look up the link key by kid (current, or inside its verify window), recompute the
   MAC in constant time, check purpose `upload` and expiry. Any failure → `404 upload_not_found`.
2. **Rate limit.** `RL_TENANT` keyed by the token's tenant ID (1,800 per 60 seconds) → `429 rate_limited`.
3. **D1 facts** (one batch, 5-second deadline, `503 unavailable` on failure): the identity's `status` and
   `mailbox_do_id`, the tenant's `status` and partner. Identity `deleting` or `deleted`, or tenant
   `erasing` or `erased` → `404 upload_not_found`; tenant `suspended` → `403 tenant_suspended`; the
   tenant's partner suspended → `403 partner_suspended`; identity `paused` → `409 identity_paused`; a
   restore freeze or a `read_only` containment → `503 unavailable` with `details.reason` `frozen` or
   `read_only` ([Errors](../../reference/errors.md#server-and-dependencies)).
4. **Framing, before reading any byte.** The handler reads the upload with `GetUpload` (no row →
   `404 upload_not_found`). `Content-Length` must be present and equal the declared size; otherwise
   `400 upload_size_mismatch` with `details.reason = "content_length"`, `details.expected` and
   `details.received` (`null` when absent). A `Content-Encoding` other than `identity` →
   `400 upload_size_mismatch` with `details.reason = "encoding"`. The request's `Content-Type` is ignored:
   the type was declared at create. Neither case is counted: nothing was read.
5. **Claim.** `MailboxRequest::BeginUpload { id, put_token, now }` with a fresh random `put_token`:
   - `pending`, or `uploading` whose lease has lapsed → claimed: `status = 'uploading'`,
     `lease_until = now + 120 s`;
   - `uploading` with a live lease → `409 upload_in_progress` (retryable), `Retry-After` = the seconds
     left on the lease (at least 1);
   - `ready` → `200` with the upload, **without reading the body** (a retried `PUT` after a lost
     response is a success, not an error);
   - `rejected` → `422 attachment_risky` with `details.risk` or `details.reason = "infected"`;
   - `expired` → `410 upload_expired` with `details.reason`; a `ready`, `pending` or `uploading` upload
     whose `expires_at` has passed is set `expired` here and answers the same;
   - no row → `404 upload_not_found`.
6. **Resume check.** `BLOBS.head(r2_key)`. An object that exists with the declared size and a SHA-256
   checksum equal to the declared one was written by an earlier attempt whose response was lost; skip
   steps 7 and 8 (the `head` answer already shows both values). An object that exists and does not match
   is deleted first.
7. **Stream into R2.** Pipe the request body through a `FixedLengthStream` of the declared size: an
   identity transform stream that accepts exactly that number of bytes and no more
   ([Workers Request › Set the Content-Length header](https://developers.cloudflare.com/workers/runtime-apis/request/),
   read 2026-10-17). The page does not say how R2's `put` treats a stream that ends early or how it
   learns the length; the first task of M33 is a real-runtime test against workerd's R2
   (`it::uploads::v_put_stream_contract`) that pins both before the handler is written, and steps 7 and
   8 below hold whichever way it answers. Then call `BLOBS.put(r2_key, stream, { sha256: declared, customMetadata })` with
   `customMetadata` `tenant`, `identity`, `upload`. R2 uses the `sha256` option to check the received
   object's integrity
   ([R2 Workers API reference › R2PutOptions](https://developers.cloudflare.com/r2/api/workers/workers-api-reference/),
   updated 2026-07-31, read 2026-10-17) and fails the `put` with `10037 BadDigest` on a mismatch, so the Worker never hashes and never buffers the file: memory
   stays at the stream's chunk size, whatever the upload's size. The handler renews the lease every
   60 seconds (`RenewUpload`). R2's error code is at the end of the error message
   ([R2 error codes](https://developers.cloudflare.com/r2/api/error-codes/), updated 2026-07-31, read
   2026-10-17):
   - `10037 BadDigest` → count a failure; `400 upload_checksum_mismatch` with `details.expected`
     (the declared SHA-256). Nothing is stored.
   - `10013 IncompleteBody`, a `FixedLengthStream` error, or the client disconnecting → count a
     failure; `400 upload_size_mismatch` with `details.reason = "incomplete"` (the client may never see
     it). Nothing is stored.
   - `10058 TooManyRequests` (R2 allows one write per second to the same key, and concurrent writes to
     one key get `429`, [R2 limits](https://developers.cloudflare.com/r2/platform/limits/), read
     2026-10-17), or any other R2 error → `FailUpload { counted: false }`; `503 unavailable`
     (retryable). A `503` caused by the service is never counted against the client.
8. **Verify what was stored.** The `put` result's `size` and `checksums.sha256` must equal the declared
   values; a mismatch (defence in depth: it should be impossible after step 7) deletes the object,
   counts a failure and answers as `10037`.
9. **Classify and scan** ([§4.3](#43-classification-and-the-scanner)).
10. **Complete.** `MailboxRequest::CompleteUpload { id, put_token, sniffed_type, risk, scan_status }`.
    Inside one transaction it re-checks that `put_token` still holds the claim and that `expires_at` has
    not passed: then `ready`, or `rejected` with `object_state = 'deleting'`. A lost claim (the lease
    lapsed and another `PUT` took over) answers `409 upload_in_progress`; the other `PUT` finishes the
    job. After commit, a rejected upload's object is deleted and the response is `422 attachment_risky`;
    otherwise `200`.

**No `complete` call.** AgentMail's Drive and S3-style flows need a third call because the bytes bypass
the API. Here the request that carries the bytes is the one that verifies and classifies them, so a
client makes two calls: create, then `PUT`. A client that lost the `PUT` response repeats the `PUT`
(step 5 answers `200` for a `ready` upload, step 6 skips re-sending bytes already stored) or reads the
upload with `GET`.

### 3.3 Read — `GET /v1/identities/{identity_id}/uploads` and `GET …/uploads/{upload_id}`

The list returns the identity's uploads, newest first, with the standard `limit` and `cursor`
([REST API › Pagination](../../reference/api.md#pagination)) and an optional `status` filter (one of the
five values). Tombstones (`rejected`, `expired`) are listed while they exist. Lists never carry
`upload_url`. `GET` of one returns the upload; while it is `pending` or `uploading` it carries a freshly
minted `upload_url`. A missing, foreign or deleted upload is `404 upload_not_found`.

There is no route that returns an upload's bytes. The bytes are read back only by the Worker, to classify
them and to copy them into messages; a sent message's attachments are read with the existing
`GET …/messages/{message_id}/attachments/{attachment_id}` (`attachments:read`).

### 3.4 Delete — `DELETE /v1/identities/{identity_id}/uploads/{upload_id}`

`204`. Deletes the row and, after commit, the object, in any state. A send that is between validating
the upload and committing holds an in-memory pin on it ([§6](#6-concurrency-and-idempotency)); a delete
then gets `409 upload_in_use` (retryable, `Retry-After: 5`). A `PUT` in progress loses its claim: its
`CompleteUpload` finds no row, deletes the object it wrote and answers `404 upload_not_found`. Deleting a
missing upload is `404 upload_not_found`. Messages that already copied the upload keep their copies.

### 3.5 Attaching an upload

Send, reply and reply-all take attachment items of two shapes; forward gains the same `attachments`
array next to `include_attachments`:

```json
"attachments": [
  { "upload_id": "upl_01JAC3Q7W9X2M5N6P8R0T1Y4ZB", "filename": "FNOL-7781.pdf", "disposition": "attachment" },
  { "filename": "logo.png", "content_type": "image/png", "content_base64": "iVBORw0KGgo…",
    "disposition": "inline", "content_id": "logo" }
]
```

- An item has exactly one of `content_base64` and `upload_id`; neither or both → `400 invalid_request`
  (`details.errors[0].path = "attachments[{i}]"`).
- With `upload_id`: `filename` is optional (the upload's filename by default) and sanitised;
  `content_type` is not accepted (`400 invalid_request`, "taken from the upload"); `disposition` and
  `content_id` are as for inline items (an `inline` part needs `content_id`).
- The upload must belong to the identity that sends. An upload of another identity, of another tenant,
  or one that does not exist is `404 upload_not_found` with `details.index`, the same answer in every
  case.
- Drafts store attachments in the same two shapes ([Drafts, approvals and scheduled send](drafts.md));
  a draft must not depend on the upload once a revision is saved (§5.4).

`Message.attachments[]` gains `upload_id`: the upload the attachment was copied from, or `null` (inline
base64, inbound, forwarded original). It survives the upload's expiry and deletion: it records where
the bytes came from, not a live reference.

## 4. The send path with uploads

### 4.1 Policy step 12, extended

Step 12 of the [policy pipeline](outbound.md#policy-pipeline) ("Content") gains these checks, in this
order, after the existing ones (text or html present, subject, at most 32 attachments, labels and
metadata):

1. **Item shape** (§3.5) → `400 invalid_request`.
2. **For each `upload_id` item, in index order**, against the mailbox's `uploads` table (this runs inside
   `Submit`, after the idempotency lookup):
   1. no row → `404 upload_not_found`, `details.index`;
   2. `pending` or `uploading` → `409 upload_not_ready` (retryable), `details.index`, `details.status`;
   3. `rejected` → `422 attachment_risky`, `details.index` and `details.risk` (or
      `details.reason = "infected"`);
   4. `expired`, or `ready` with `expires_at ≤ now` → `410 upload_expired`, `details.index`,
      `details.reason`;
   5. the final filename (the item's, sanitised, or the upload's) is checked again with
      `core::attach::classify_name(final_filename, sniffed_type)`: the executable-extension list, the
      macro extensions, and `type_mismatch` between the new extension and the sniffed family. A risk →
      `422 attachment_risky`, `details.index`, `details.risk`. Renaming `report.pdf` to `report.exe`
      after upload never bypasses the rules;
   6. pin the upload (§6).
3. **Inline base64 items are classified too.** `core::attach::sniff` and `classify_risk` run on each
   decoded `content_base64` item, with its filename and declared type. A quarantining risk →
   `422 attachment_risky`, `details.index`, `details.risk`. This is new for inline attachments: without
   it, an agent could avoid the upload checks by switching to base64. The malware scanner is not called
   for inline items, because the submit path has a 30-second deadline and the scanner may take 30
   seconds per file ([Inbound › Malware scanner hook](inbound.md#malware-scanner-hook-pm_scanner_url-p1));
   uploads are scanned when their bytes arrive.
4. **Total size.** The decoded sizes of all attachments (inline bytes and uploads' `size`) must be at most
   26,214,400 bytes (a Pylota Mail choice: the upload maximum, so one maximal upload always fits);
   otherwise `413 message_too_large` with `details.reason = "attachments_total"`, `details.size` and
   `details.max`, whatever `large_attachments` says. The existing limit of 32 attachments still applies.

A failure at any of these steps unpins every upload pinned so far.

### 4.2 Step 17: size, inline or linked

Step 17 ("Size after composition") is decided **from sizes alone**, before any upload byte is read:
`core::compose::encoded_size(size)` gives each part's base64 size with 76-character lines, and the
composed size is the headers and bodies plus those parts. When the result is above
5,234,688 bytes (5,242,880 − 8,192, [Outbound › Attachments and size](outbound.md#attachments-and-size)):

- `large_attachments: "refuse"` → `413 message_too_large`, `details.reason = "transport_limit"`;
- `large_attachments: "link"` → the existing rule: attachments are replaced by signed links, largest
  first, until the message fits. Uploads and inline items are treated alike.

The result is a plan: which attachments are embedded in the MIME and which are linked. A dry run
(`?dry_run=true`) stops here: it validates the uploads and computes the plan, reads no upload bytes,
copies nothing and pins nothing past the request.

### 4.3 Classification and the scanner

Used by step 9 of the `PUT` (§3.2):

1. **Memory budget.** Classification reads the stored object back into memory: `core::attach` inspects
   the first 8 KiB, ZIP central directories, OLE directories and PDF trailers
   ([Inbound › Sniffing](inbound.md#sniffing)), which can sit anywhere in the file. A Worker isolate has
   128 MB, shared by every request it is running
   ([Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17), so
   each isolate keeps a byte budget of 33,554,432 bytes (32 MiB, a Pylota Mail choice: a quarter of the
   isolate) for upload bytes held for classification. A classification waits up to 10 seconds for room;
   then the `PUT` gets `503 unavailable` (retryable, not counted), the object stays in R2, and the retry
   skips streaming (§3.2 step 6).
2. **Classify.** `core::attach::sniff` and `classify_risk` with the declared filename and content type:
   the rules of [Inbound › Risk classification](inbound.md#risk-classification), unchanged.
3. **Scan.** When `PM_SCANNER_URL` is set and there is no quarantining risk, the bytes are posted to the
   scanner with the contract of [Inbound › Malware scanner hook](inbound.md#malware-scanner-hook-pm_scanner_url-p1)
   (headers `X-Pylota-Attachment-Sha256`, `X-Pylota-Sniffed-Type`; 30-second timeout; no redirects;
   response read up to 4 KB; the SSRF guard). Every upload is scanned, up to the 26,214,400-byte maximum
   (the inbound rule scans attachments up to 20 MB; uploads are scanned whole because they are the files
   an agent chose to send). `clean` → `scan_status = 'clean'`; `infected` → `rejected` with
   `status_reason = 'infected'`; `error`, a timeout, a non-2xx status or an unparseable body →
   `scan_status = 'error'`, the upload becomes `ready`, and `upload_scanner_error_total` is incremented.
   This is the inbound rule (fail open on scanner errors), so a scanner outage never stops sending.
4. Release the memory budget.

### 4.4 Steps 6 and 7 of the reservation

[Reservation inside the mailbox](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1), with uploads:

- **Step 4 (composition in memory).** The embedded uploads are read from R2 (`BLOBS.get`); by step 17
  they total at most the composed limit, so they fit in memory as inline base64 attachments already do.
  A missing object (only possible after a restore, [V36](../edge-cases.md)) sets the upload `expired`
  (`object_missing`), releases the reservations and returns `410 upload_expired`.
- **Step 6 (sent copy).** Besides `out/{msg}.eml`, every attachment that came from an upload is copied to
  `t/{ten}/i/{idn}/out/{msg}/a/{att}`, the existing key of outbound attachments: R2 to R2, streamed
  through a `FixedLengthStream` of the upload's size with the `sha256` option, so the copy is verified
  like the upload was and never held in memory. A linked attachment's link points at that copy. Any
  failure takes the existing path: everything written so far is deleted, the reservations are released,
  and the send gets `503 unavailable`; a copy that runs past the submit deadline gets `504 timeout`
  before anything is committed, so a retry with the same key is safe.
- **Step 7 (one transaction).** Re-check that each pinned upload is still `ready` (it must be: the pin
  blocks delete and expiry), insert the `attachments` rows with `upload_id`, `sha256` and `sniffed_type`
  from the upload, and `UPDATE uploads SET use_count = use_count + 1, last_used_at = ?now` for each
  distinct upload. If the transaction fails, the copies are deleted with the sent copy (best effort,
  [I9](../edge-cases.md)) and `use_count` is unchanged.
- **Content-Type of the part.** The declared type, except that a declared `application/octet-stream`
  with a recognised `sniffed_type` is sent as the sniffed type.
- Every exit path unpins.

**Idempotency comes first.** A replay of an accepted send returns the stored response before step 12
runs, so it succeeds even after the upload expired or was deleted ([V25](../edge-cases.md)): the message
already holds its own copies.

## 5. Storage

### 5.1 Mailbox table

Added to the `IdentityMailbox` schema with the idempotent statements of
[Design § 4, rule 6](index.md#4-durable-object-transactions) (the schema version number is assigned at
integration, after the other parity pages' additions):

```sql
CREATE TABLE IF NOT EXISTS uploads (
  id            TEXT PRIMARY KEY,                          -- upl_
  filename      TEXT NOT NULL,                             -- sanitised: no path, ≤ 255 bytes
  content_type  TEXT NOT NULL,                             -- as declared, ≤ 127 bytes
  size          INTEGER NOT NULL CHECK (size BETWEEN 1 AND 26214400),
  sha256        TEXT NOT NULL,                             -- lower-case hex, 64 characters
  status        TEXT NOT NULL CHECK (status IN ('pending','uploading','ready','rejected','expired')),
  status_reason TEXT CHECK (status_reason IN ('risk','infected','expired','too_many_attempts','object_missing')),
  sniffed_type  TEXT,
  risk          TEXT CHECK (risk IN ('executable','macro','encrypted_archive','archive_bomb',
                                     'type_mismatch','encrypted_document')),
  scan_status   TEXT NOT NULL DEFAULT 'skipped' CHECK (scan_status IN ('skipped','clean','infected','error')),
  r2_key        TEXT NOT NULL,                             -- t/{ten}/i/{idn}/up/{upl}
  object_state  TEXT NOT NULL DEFAULT 'absent' CHECK (object_state IN ('absent','present','deleting')),
  put_token     TEXT,                                      -- the PUT holding the claim (status uploading)
  lease_until   INTEGER,                                   -- the claim lapses after this time
  failed_puts   INTEGER NOT NULL DEFAULT 0,                -- counted failures; the 10th expires the upload
  use_count     INTEGER NOT NULL DEFAULT 0,
  last_used_at  INTEGER,
  created_at    INTEGER NOT NULL,
  expires_at    INTEGER NOT NULL,
  ended_at      INTEGER                                    -- when it became rejected or expired
);
CREATE INDEX IF NOT EXISTS uploads_status_expiry ON uploads(status, expires_at);
CREATE INDEX IF NOT EXISTS uploads_created ON uploads(created_at, id);

-- attachments gains the upload a sent attachment was copied from (NULL for inline base64 and inbound):
ALTER TABLE attachments ADD COLUMN upload_id TEXT;        -- upl_; not a foreign key: the upload may be gone
```

SQLite has no `ADD COLUMN IF NOT EXISTS`, so the migration runs the `ALTER TABLE` only when
`pragma_table_info('attachments')` has no `upload_id` row, which keeps the migration idempotent under
rule 6. `meta` gains `alarm:uploads` (the earliest pending upload wake-up). Every column has a writer and
a reader: the routes and `CompleteUpload` write the row; the routes, step 12, the alarm, the storage
report and erasure read it; `attachments.upload_id` is written at step 7 and read by the Message object
and by erasure (§9).

R2 gains one key, added to [Data model § 4](data-model.md#4-r2-objects):

| Key | Content | Custom metadata | Deleted by |
|---|---|---|---|
| `t/{ten}/i/{idn}/up/{upl}` | Upload bytes, verified against the declared SHA-256 | `tenant`, `identity`, `upload` | Rejection, expiry, `DELETE`, erasure; the orphan sweep after 15 days when no row names it |

### 5.2 Mailbox operations

New `MailboxRequest` variants; each runs one `transaction_sync`:

| Operation | Does |
|---|---|
| `CreateUpload { new_upload, max_live: 100, max_live_bytes: 524288000 }` | The limit checks and the insert (§3.1 step 5); arms `alarm:uploads` for `expires_at` → `Created(Upload)` or `LimitReached { limit, used, max }` |
| `GetUpload { id }`, `ListUploads { status, after, limit }` | Reads |
| `DeleteUpload { id }` | `InUse` when pinned; otherwise deletes the row and returns the key for the caller to delete after commit → `Deleted { r2_key }`, `InUse`, `NotFound` |
| `BeginUpload { id, put_token, now }` | §3.2 step 5 → `Claimed(Upload)`, `InProgress { retry_after }`, `Ready(Upload)`, `Rejected(Upload)`, `Expired(Upload)`, `NotFound` |
| `RenewUpload { id, put_token, now }` | `lease_until = now + 120 s` while `put_token` holds the claim → `Renewed` or `Lost` |
| `FailUpload { id, put_token, counted }` | Back to `pending`; `failed_puts + 1` when counted; the 10th sets `expired` (`too_many_attempts`, `object_state = 'deleting'`) |
| `CompleteUpload { id, put_token, sniffed_type, risk, scan_status, now }` | §3.2 step 10 → `Ready(Upload)`, `Rejected(Upload)`, `Expired(Upload)`, `Lost`, `NotFound` |
| `UploadPlan { after, limit }`, `EraseUploads { ids }` | For erasure of a held identity (§9): lists the uploads with their keys, then deletes the rows → count |

The `Submit` path reads and updates `uploads` inside the mailbox; it needs no new operation. Drafts, which
also live in the mailbox, copy an upload with the internal function `uploads::copy_to(id, dest_key)`
(the step 6 copy), not through an RPC.

### 5.3 The `alarm:uploads` purpose

The mailbox's single alarm ([Design § 4, rule 5](index.md#4-durable-object-transactions)) gains
`alarm:uploads`, set to the earliest of: the `expires_at` of a `pending`, `uploading` or `ready` upload,
`ended_at + 7 days` of a tombstone, and `now + 60 s` while any row has `object_state = 'deleting'`. The
handler:

1. sets `expired` (`status_reason = 'expired'`, `ended_at = now`, `object_state = 'deleting'` when the
   object may exist) on every live upload whose `expires_at` has passed and that is not pinned (a pinned
   one is retried 60 seconds later);
2. after commit, deletes the objects of `deleting` rows and sets them `absent` (a failed delete stays
   `deleting` and is retried);
3. deletes tombstones whose `ended_at` is more than 7 days old and whose object is `absent`;
4. re-arms.

The daily `alarm:maintenance` runs the same handler, so a missed alarm is caught within a day, and
`MailboxRequest::Hygiene` (the tenant retention job's `hygiene` step,
[Privacy § 5.2](privacy.md#52-steps-of-a-tenant-retention-job)) runs it too.

### 5.4 Reuse: copy, never reference

A `ready` upload can be attached by any number of sends of its identity until it expires. Each accepted
message gets its own copy (§4.4), and each draft revision that references an upload copies it when the
revision is saved, so:

- deleting or expiring an upload never changes a sent message, a queued message waiting for transport, or
  a saved draft;
- erasing a message erases its copy without touching other messages that used the same upload;
- `storage_gb` counts each copy, which is what is stored.

The cost is storage: one 25 MiB file sent to ten separate threads is stored ten times (plus the upload
until it expires). A shared, reference-counted object would save that, but it would couple the lifetime
of every message to every other message that used the file, and an erasure of one message could not
delete bytes another message still points to. Pylota Mail chooses the simpler rule.

## 6. Concurrency and idempotency

- **One mailbox, one writer.** Every state change is a `transaction_sync` inside the identity's
  `IdentityMailbox`, so two creates racing for the 100th place, or a `DELETE` racing a `PUT`, are
  ordered.
- **One `PUT` at a time per upload.** The claim (`put_token`, `lease_until`) admits one `PUT`; a second
  gets `409 upload_in_progress` with `Retry-After`. R2 would refuse concurrent writes to one key anyway
  ([R2 limits](https://developers.cloudflare.com/r2/platform/limits/), read 2026-10-17); the claim makes
  the answer deterministic instead of a race between two streams.
- **A lost claim.** If a `PUT`'s lease lapses (the Worker was evicted mid-stream) and another `PUT`
  claims the upload, the first one's `RenewUpload` or `CompleteUpload` answers `Lost`; it stops and
  answers `409 upload_in_progress`. If both wrote the object, both wrote the same verified bytes (R2
  checked the same declared SHA-256), so whichever completes leaves a correct object.
- **Retried `PUT`s.** After `ready`: `200`, body not read. After a write whose response was lost: the
  `head` check (§3.2 step 6) finds the verified object and skips the stream.
- **Pins.** The mailbox keeps `upload_pins: HashMap<String, u32>` in memory. Step 12 pins each upload a
  send uses; every exit path of `Submit` unpins (a guard, as for `in_flight`). `DeleteUpload` answers
  `InUse` for a pinned upload, and the expiry handler skips it. A Durable Object has one live instance
  and loses its memory only together with every request it was running
  ([Reservation inside the mailbox](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1)), so a pin can
  never outlive its send.
- **Create idempotency.** `Idempotency-Key` on create uses the existing per-key, per-tenant records
  ([REST API › Idempotency](../../reference/api.md#idempotency)): the record stores the upload ID; a
  replay re-reads the upload and mints a new URL; the same key with a different body gets
  `409 idempotency_conflict`. Without a key, a retried create makes a second `pending` upload that
  expires unused.
- **Send idempotency.** Unchanged: the key is checked before step 12 (§4.4), and the fingerprint covers
  the request body, which names the `upload_id`, so a retry with the same key and body attaches the same
  bytes. A draft sent with `draft:{draft_id}:{revision}` attaches the revision's copies.

## 7. Failure handling

| Failure | Behaviour |
|---|---|
| D1 unavailable (create or `PUT`) | `503 unavailable`; nothing written; a `PUT` is not counted |
| `TenantQuota` unavailable (create's storage gate) | `503 unavailable`; nothing written |
| Mailbox call fails or times out (create) | `503 unavailable`; a create that committed but whose answer was lost leaves a `pending` upload that expires unused, or that an `Idempotency-Key` replay returns |
| Client disconnects mid-`PUT` | R2 stores nothing (`IncompleteBody` or the stream error); the failure is counted; the upload returns to `pending` |
| Worker evicted mid-`PUT` | The lease lapses after at most 120 seconds; the next `PUT` claims; an object that was fully written is found by the `head` check |
| R2 refuses the write (`10058`, `5xx`) | `503 unavailable`, not counted |
| R2 digest mismatch (`10037`) | `400 upload_checksum_mismatch`, counted, nothing stored |
| Classification has no memory room within 10 s | `503 unavailable`, not counted; object kept; the retry skips the stream |
| Scanner error, timeout, non-2xx, bad body | `scan_status = 'error'`, upload `ready`, `upload_scanner_error_total` incremented |
| `CompleteUpload` fails after the object was written | `503 unavailable`; the upload is still `uploading` until the lease lapses, then `pending`; the retry's `head` check finds the object and only classification runs again |
| Object delete fails (rejection, expiry, `DELETE`) | `object_state` stays `deleting` (for `DELETE`, the row is gone and the object is left to the orphan sweep after 15 days); the alarm retries every 60 seconds |
| Upload object missing when a send reads or copies it | The upload is set `expired` (`object_missing`); the send gets `410 upload_expired`; `uploads_expired_total{reason=object_missing}` |
| R2 copy fails or times out at submit step 6 | The existing step 6 rule: everything written is deleted, reservations released, `503 unavailable` or `504 timeout`; nothing committed |
| Step 7 transaction fails | Copies and sent copy deleted (best effort, I9); `use_count` unchanged |

## 8. Security and visibility

- **Permissions.** Either `messages:send` or `drafts:write`. `RouteSpec` gains
  `any_permissions: &'static [Permission]` (when not empty, the key must hold at least one of them),
  checked at step 2 of [Security § 5.2](security.md#52-order-of-checks) like `permissions`; a key with
  neither gets `403 permission_denied` with `details.required = ["messages:send", "drafts:write"]` and
  `details.any_of = true`. The check reads only the key and the path, so it leaks nothing.
- **Key levels.** Every level can use the routes within its scope. An identity key reaches only its own
  identity's uploads; another identity's upload ID in the path or in an attachment item is
  `404 upload_not_found`, as for a missing one. A partner key reaches uploads only of its own tenants'
  identities; a tenant of another partner answers as missing ([Security › Partner keys](security.md#partner-keys)).
- **Same identity only.** An upload is attachable only by sends of the identity that owns it, whatever
  key level sends. A tenant key cannot attach identity A's upload to a send from identity B; it uploads
  again for B. This keeps every upload inside one mailbox, one erasure scope and one storage report.
- **The upload URL.** A bearer capability for one upload's declared bytes, valid for at most one hour,
  signed by the `link` key ([Security § 7.3](security.md#73-signed-links)). It cannot read anything,
  cannot write other bytes than those declared, and stops working when the upload leaves `pending` or
  `uploading`. Rotating the link key invalidates outstanding upload URLs after the key's verify window,
  as for attachment links; `GET` of the upload mints a new one. The URL is never logged (the token is
  replaced by its first 8 characters in logs, as for other signed links).
- **What the `PUT` checks without an API key.** Everything a keyed write checks about the target: the
  identity and tenant state, the partner's suspension, the freeze and containment states (§3.2 step 3).
  A suspended tenant cannot finish an upload begun before the suspension.
- **Untrusted content.** `filename` is untrusted: it is sanitised, never used as an R2 key, path, header
  value or log field, and is returned like an attachment filename. The R2 key uses only IDs.
- **Classification before use.** No upload byte reaches a recipient before `core::attach` (and the
  scanner, when configured) has seen it. A rejected upload's bytes are deleted at once. Inline base64
  attachments are classified too (§4.1).
- **No fetches.** The Worker never makes an outbound request on behalf of an upload except to the
  operator's scanner (`PM_SCANNER_URL`, fixed by the operator, under the SSRF guard).
- **Attack suite.** The four routes and `PUT /v1/links/{token}` join `it::security::cross_tenant_matrix`
  ([Testing § 7](testing.md#7-cross-tenant-attack-suite)): a foreign upload ID in every route and in an
  attachment item answers exactly as a missing one; an upload token of tenant A replayed with an upload
  ID of tenant B fails the MAC.

## 9. Privacy

**Inventory.** One new store: mailbox `uploads` rows and their R2 objects under `t/{ten}/i/{idn}/up/`.
They hold file contents the agent chose to send, and filenames. Each lives at most 7 days (less when the
tenant's or identity's retention is shorter, §3.1 step 6), plus a 7-day tombstone that holds metadata
only (filename, size, digest, state), never bytes. [Privacy § 2](privacy.md#2-data-inventory) gains the
row.

**Retention.** Uploads are not mail, so the message retention cutoffs do not select them; their own expiry
deletes them (§5.3), and the expiry is capped by `retention.message_days` and `retention.message_hours`
so an upload never outlives the mail retention its identity has. The tenant retention job's `hygiene`
step runs the same sweep as a backstop.

**Erasure.** Every scope deletes uploads, and the receipt counts them in a new field
`uploads_deleted` (rows deleted); their objects count in `r2_objects_deleted` as today.

| Scope | What happens to uploads |
|---|---|
| Message, thread | `ErasurePlan` also returns, for each message to erase, the `r2_key` of every upload its attachments were copied from (`attachments.upload_id`) that still exists; the job deletes those objects with the message's keys, and `EraseRows` deletes those upload rows in the same transaction and returns their count. The upload is deleted even if other messages used it: their copies are their own (§5.4), and a file sent to the person whose data is erased should not stay staged for more sends |
| Counterparty | As message scope, for every message that matches the counterparty |
| Identity (no holds) | The prefix delete of `t/{ten}/i/{idn}/` removes the objects; `CountAll` counts the `uploads` rows for the receipt before `WipeAll` |
| Identity (holds) | Uploads are never held: a legal hold covers threads, and an upload belongs to none. After the held-thread pass, `UploadPlan` and `EraseUploads` delete every upload of the identity and its objects |
| Tenant | Covered by the identity steps and `sweep_r2` |
| People (console accounts) | No upload is tied to a person |

A message erased by any scope also loses its own attachment copies (`out/{msg}/a/{att}`), as today.

**Legal holds.** Not applicable to uploads (above). A held thread keeps its messages' attachment copies,
which is where sent bytes live.

**Subject-access export.** Uploads are not exported: they are a staging area that empties within 7 days,
not correspondence, and the bytes of every upload that was sent are in the exported messages'
attachments. [Privacy § 11](privacy.md#11-what-remains-after-deletion) lists unsent uploads as not
exported.

**Backup bucket.** The nightly `backup` job skips keys under `…/up/`
([Privacy § 5.4](privacy.md#54-optional-r2-backup-copy)): restoring an upload is never useful, and a copy
would outlive the upload's expiry. Retention and erasure deletes still go to both buckets, as for every
`t/` key.

**Orphans.** The mailbox's rolling orphan sweep ([Inbound › Orphan objects](inbound.md#orphan-objects))
names `uploads.r2_key` among the keys that keep an object alive; an object under `up/` that no row names
is deleted once it is 15 days old.

**Restore.** A point-in-time restore can bring back `uploads` rows whose objects were deleted since. The
first `PUT`, attach or alarm that finds the object missing sets the upload `expired`
(`object_missing`); nothing is sent with missing bytes. A re-applied erasure
([Privacy § 6](privacy.md#6-erasure)) deletes restored upload rows by the same rules.

**Logs.** Upload ID, size, status, result codes and durations; never the filename, the token or the
digest.

## 10. Billing and limits

- **Storage.** Upload bytes count toward `storage_gb`. The mailbox's storage report
  ([Plans, metering and billing › Storage](billing.md#storage)) adds `SUM(size)` of `uploads` rows whose
  `object_state` is `present` or `deleting`. `CompleteUpload` sets `present` for `ready`; rejection,
  expiry and deletion set `deleting` until the object is gone.
- **Gate.** Create is refused with `402 billing_limit` (`storage_gb`) while storage is at or over its
  allowance (§3.1 step 4); a `PUT` is not gated, because its create already was and the bytes were
  declared. Sends with attachments are gated at policy step 18 as today.
- **No new allowance.** Uploads are not a plan feature; there is no count to buy.
- **Limits (Pylota Mail choices).** 26,214,400 bytes per upload; 100 live uploads and 524,288,000
  declared bytes of live uploads per identity (`422 upload_limit_reached`); `expires_in` 900–604,800
  seconds, default 86,400; upload URL 3,600 seconds; 10 counted failed `PUT`s; 26,214,400 decoded
  attachment bytes per message; tombstones 7 days. They are fixed, not policy: they bound memory, storage
  and work per identity, and no tenant needs them higher for staging.
- **Test tenants.** Uploads behave the same; the send's recipients are already limited to the simulator
  and loopback (policy step 8).

## 11. Events and audit

No event and no audit row. An upload is a step inside a client's own request flow: the client learns
every outcome from the `PUT` response or a `GET`, and the result that matters to others (a sent message
with attachments) is already evented as `message.sent`. Uploads can be created at a high rate, so audit
rows would bury the decisions the audit log exists for. The send's own events and the message's
`attachments[].upload_id` record which upload a message used.

## 12. Console

No uploads screen. Uploads are a programmatic staging step with a lifetime of hours, and the console never
sends mail with attachments. Their bytes appear in the workspace's storage meter on `/console/plan` and the
Overview through the storage report. A sent message's attachments are shown and downloaded as today.

## 13. MCP and CLI

**MCP.** A model cannot stream a file through MCP: tool arguments are JSON, and a file is base64 text the
model must produce (§1). So:

- `mail_send` and `mail_reply` accept attachment items of both shapes (`oneOf`), and `mail_forward`
  gains `attachments` with both shapes. Small files the model already holds as text (a generated CSV, a
  short note) can still go inline. The MCP body limit stays 7 MiB.
- **`mail_create_upload`** (`messages:send` or `drafts:write`, `RL_API`): arguments `identity`,
  `filename`, `content_type`, `size`, `sha256`, optional `expires_in` and `idempotency_key`; calls
  `POST /v1/identities/{id}/uploads`. Its description tells the model how to finish: "Then send the file
  bytes with an HTTP PUT to upload_url (for example `curl -sS -T FILE -H 'Content-Type:
  application/octet-stream' "$UPLOAD_URL"`) from a tool that can read the file, and pass upload_id in
  attachments. The URL works for one hour and only for these exact bytes." Annotations:
  `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: false` (`true` only when
  `idempotency_key` is given; MCP annotations are per tool, so it is declared `false`),
  `openWorldHint: false`.
- **`mail_get_upload`** (`messages:send` or `drafts:write`, `RL_API`): argument `upload_id` (and
  `identity`); calls `GET …/uploads/{upload_id}`; read-only annotations. It returns a fresh `upload_url`
  while the upload is `pending` or `uploading`.
- No tool deletes an upload: an unused upload expires on its own, and a model has no reason to delete
  one.
- In `tools/list`, both tools appear when the key holds `messages:send` or `drafts:write`
  ([MCP § 3](mcp.md#3-authentication-and-tool-filtering)).

**CLI.**

```text
pmail uploads create FILE [--identity ID] [--filename NAME] [--content-type TYPE] [--expires-in SECONDS]
pmail uploads list [--identity ID] [--status STATUS]
pmail uploads get UPLOAD_ID [--identity ID]
pmail uploads delete UPLOAD_ID [--identity ID]
```

`create` reads the file, computes its size and SHA-256 locally, creates the upload, `PUT`s the bytes
(retrying the `PUT` on `503` and `409 upload_in_progress` after `Retry-After`, at most 5 times), and
prints the upload ID and `expires_at`. The content type defaults to the file extension's
(`application/octet-stream` when unknown). `pmail send --attach FILE` (and `reply`, `forward`) sends files
of at most 1,048,576 bytes inline and uploads larger ones first (a Pylota Mail choice: below 1 MiB the
extra round trips cost more than the base64).

## 14. Observability

New metrics ([Observability § 3.2](observability.md#32-catalogue)):

| Metric | Kind | Labels | Emitted by |
|---|---|---|---|
| `uploads_created_total` | counter | – | `handlers/uploads.rs` |
| `upload_puts_total` | counter | result: `ready`, `already_ready`, `rejected_risk`, `rejected_infected`, `size_mismatch`, `checksum_mismatch`, `incomplete`, `in_progress`, `expired`, `not_found`, `unavailable` | `handlers/links.rs` |
| `upload_bytes` | observation | – | `handlers/links.rs`, per `ready` upload |
| `upload_put_ms` | observation | – | `handlers/links.rs`: request start → response |
| `upload_classify_wait_total` | counter | result: `immediate`, `waited`, `timed_out` | `uploads/classify.rs` |
| `upload_scanner_error_total` | counter | – | `uploads/classify.rs` |
| `uploads_expired_total` | counter | reason: `expired`, `too_many_attempts`, `object_missing` | `mailbox/uploads.rs` |
| `upload_attachments_total` | counter | mode: `inline`, `link` | `mailbox/submit.rs` |
| `upload_copy_ms` | observation | – | `uploads/copy.rs`, per copied upload |

Alert ([Observability § 5.3](observability.md#53-alert-list)): `upload_object_missing` (class B, ticket)
when `uploads_expired_total{reason=object_missing}` > 0 while no restore freeze is in effect: an object
went missing without a restore, which means a delete bug. Runbook: compare the mailbox's `uploads` rows
with `BLOBS.list` of the identity's `up/` prefix and the orphan sweep's log.

The Outbound and delivery dashboard gains: `upload_puts_total` by result, `upload_put_ms` and
`upload_copy_ms` p50/p95/p99, and inline against linked attachments.

## 15. Pylota agentic OS

**Who uses uploads.** Pylota's senders that carry files, all from its API server, never from an agent
directly (Pylota's integration spec, `docs/plans/pylota-mail-integration.md` in the Pylota repository,
§ 7.3): row 10 (recovery letters, a generated PDF), row 16 (FNOL packs to insurers: forms and
photos), row 20 (insurer invoices). They run under the operator identity keys K3 (identity level,
`messages:send`, which is enough) or the platform tenant key K5 for Pylota-owned identities. Spec § 7.6
says no sender carries attachments over 5 MiB today; FNOL packs with photos are the first that will.

**How.** The send service in Pylota's API server, for each file over 1 MiB: computes SHA-256 from the
bytes it already holds in its own store, creates the upload with an idempotency key
`pm:v1:upload:{pendingActionId}:{fileSha256[0..16]}`, `PUT`s the bytes, then sends with
`{ "upload_id": … }` and the sender's existing `Idempotency-Key`. Pylota stores the upload ID and
`expires_at` on its pending action (`mail_outbound_messages` row or the pending-action record), and
re-uses the upload for a retry of the same action until it expires.

**Agents.** Specialists never handle bytes or upload URLs: they pass Pylota file references to Pylota's
capability route, which uploads server-side. The public-webchat coordinator never reaches uploads at all,
and because Pylota Mail fetches no URLs, text from a webchat or an email can never make the service fetch
and attach a file. An MCP client of Pylota's own (an operator using Claude with Pylota Mail directly) uses
`mail_create_upload` and its code tool for the `PUT`.

**Failures in Pylota.** `422 attachment_risky` fails the pending action with a visible reason in the app
("attachment refused: {risk}"), never retried. `410 upload_expired` and `404 upload_not_found` re-upload
once and resend with the same send key: a refused send records nothing, so the key is still free even
though the body now names a new `upload_id`
([Reservation inside the mailbox](outbound.md#reservation-inside-the-mailbox-fr-out-1-g1)).
`413 message_too_large` on an operator tenant with `large_attachments: "refuse"` is shown as "attachment
too large for email" with the option to send as links. `409 upload_not_ready` never happens in Pylota's
flow, because it sends only after the `PUT` returned `ready`.

**What Pylota's integration spec must add** (for its owner):

1. § 7.1 (router contract): uploads for files over 1 MiB, the create, `PUT` and send order, the upload
   idempotency key, and the error mapping above (`attachment_risky`, `upload_expired`,
   `upload_not_found`, `upload_not_ready`, `upload_limit_reached`, `413 message_too_large` with
   `details.reason`).
2. § 7.6: replace "attachments over 5 MiB: none today" with the decision on `large_attachments` per
   tenant kind (operator tenants and the platform tenant), since insurers may not accept links that
   expire after `link_ttl_hours`.
3. § 4.6 (what Pylota stores): the upload ID and `expires_at` on the pending action.
4. § 6: no key change (K3 and K5 already hold `messages:send`); K7 needs none (the auth tenant sends no
   files).
5. § 9 (erasure): a renter counterparty erasure also deletes uploads staged for that renter's letters
   (receipt `uploads_deleted`); Pylota's own file store is erased by Pylota.
6. W5.5 tests: an FNOL pack over 5 MiB sent with links, a risky file refused, a retry after expiry.

## 16. Functional requirements

To add to [PRD § 6](../prd.md) as a new group, "Uploads":

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

## 17. Edge cases

Section V of the [edge-case register](../edge-cases.md):

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| V1 | Invalid create: `size` 0 or above 26,214,400; `sha256` not 64 hexadecimal characters; `filename` empty after sanitising or above 255 bytes; `content_type` not `type/subtype` or above 127 bytes; `expires_in` outside 900–604,800 | `400 invalid_request` with `details.errors[]`; nothing stored | S | `core::uploads::validate_create`, `it::uploads::v1_invalid_create` |
| V2 | `PUT` body ends early: the client disconnects or sends fewer bytes than its `Content-Length` | R2 stores nothing; the failure is counted; the upload returns to `pending`; the client (if still connected) gets `400 upload_size_mismatch` with `reason = "incomplete"`; a full `PUT` afterwards succeeds | S | `it::uploads::v_put_stream_contract`, `it::uploads::v2_partial_body` |
| V3 | `PUT` without `Content-Length` (chunked), with a `Content-Length` other than the declared size, or with a `Content-Encoding` | `400 upload_size_mismatch` (`reason` `content_length` or `encoding`, `details.expected`, `details.received`) before any byte is read; not counted | S | `it::uploads::v3_framing` |
| V4 | The bytes do not match the declared SHA-256 | R2 refuses the write (`10037`); `400 upload_checksum_mismatch` with `details.expected`; nothing stored; counted | S | `it::uploads::v_put_stream_contract`, `it::uploads::v4_checksum_mismatch` |
| V5 | Two `PUT`s of one upload at once | One claims; the other gets `409 upload_in_progress` with `Retry-After` ≥ 1; the stored object has the declared digest | S | `core::uploads::state_machine`, `it::uploads::v5_concurrent_puts` |
| V6 | `PUT` repeated after the upload is `ready` (the first response was lost) | `200` with the upload; the body is not read; no second classification or scan | S | `it::uploads::v6_put_after_ready` |
| V7 | `PUT` after `expires_at`, after the tombstone was purged, or with a URL older than one hour | Past `expires_at`: the upload is set `expired` and the `PUT` gets `410 upload_expired`; purged tombstone or expired token: `404 upload_not_found` | S | `it::uploads::v7_put_after_expiry` |
| V8 | A token that is altered, signed by an unknown or retired kid, of purpose `att` or `export` on `PUT`, or of purpose `upload` on `GET` | `404` with the token's target code (`upload_not_found` for an upload token), the same body whatever the cause | S | `core::uploads::token_purpose`, `it::uploads::scope_matrix` |
| V9 | The Worker is evicted mid-`PUT` | The claim lapses within 120 seconds; the next `PUT` claims it; an object stored complete before the eviction is found by the `head` check and not streamed again | S | `it::uploads::v9_crash_and_resume` |
| V10 | R2 refuses the write: one write per second per key (`10058`), concurrent writes, or a server error | `503 unavailable` (retryable); not counted against the client | S | `it::uploads::v10_r2_errors` |
| V11 | No classification memory room for 10 seconds | `503 unavailable`, not counted; the object stays; the retry skips the stream and only classifies | S | `it::uploads::v11_classify_budget` |
| V12 | The bytes carry a quarantining risk (`executable`, `macro`, `archive_bomb`, `encrypted_archive`, `type_mismatch`) | `rejected` (`risk`), object deleted at once; the `PUT` gets `422 attachment_risky` with `details.risk`; every later attach gets the same | S | `core::attach::b10_*`, `it::uploads::v12_risky_rejected` |
| V13 | The bytes are an `encrypted_document` | `ready` with `risk = "encrypted_document"`; attachable and sent | S | `it::uploads::v12_risky_rejected` |
| V14 | The scanner answers `infected` | `rejected` (`infected`), object deleted; `422 attachment_risky` with `details.reason = "infected"` | S | `it::uploads::v14_scanner_verdicts` |
| V15 | The scanner errors, times out, answers non-2xx or an unparseable body | `ready` with `scan_status = "error"`; `upload_scanner_error_total` incremented; sending is not blocked | S | `it::uploads::v14_scanner_verdicts` |
| V16 | A send attaches an upload of another identity, of another tenant, or one that does not exist | `404 upload_not_found` with `details.index`, the same body in all three cases; nothing stored | S | `it::uploads::v16_cross_identity_attach` |
| V17 | A send attaches an upload that is `pending` or `uploading` | `409 upload_not_ready` (retryable) with `details.index` and `details.status` | S | `it::uploads::v17_attach_states` |
| V18 | A send attaches an `expired` upload, a `rejected` one, or a `ready` one whose `expires_at` passed before the alarm ran | `410 upload_expired` with `details.reason`; `422 attachment_risky`; `410 upload_expired` | S | `it::uploads::v17_attach_states` |
| V19 | The attach item renames the upload into a risky name (`report.pdf` → `report.exe`, `.docm`, or an extension of a different family from the sniffed one) | `422 attachment_risky` with `details.index` and `details.risk`; nothing sent | S | `core::attach::v19_final_name_reclassified`, `it::uploads::v19_rename_refused` |
| V20 | An item with both `content_base64` and `upload_id`, with neither, or with `upload_id` and `content_type` | `400 invalid_request` with `details.errors[0].path` naming the item | S | `it::uploads::v20_attachment_item_shape` |
| V21 | The same upload twice in one send | Both attached; each is its own copy and attachment ID; `use_count` rises by 1 | S | `it::uploads::v20_attachment_item_shape` |
| V22 | Attachments total more than 26,214,400 decoded bytes | `413 message_too_large` with `reason = "attachments_total"`, also with `large_attachments: "link"` | S | `it::uploads::v22_totals_and_links` |
| V23 | The composed message is above the transport limit | `refuse`: `413 message_too_large` (`transport_limit`). `link`: the largest attachments become links until it fits; the linked copy is verified by SHA-256; the link serves bytes whose digest equals the upload's | S | `core::compose::v23_link_plan_from_sizes`, `it::uploads::v22_totals_and_links` |
| V24 | `DELETE`, or expiry, while a send between step 12 and its commit holds the upload | `DELETE` gets `409 upload_in_use` (retryable); expiry waits 60 seconds; the send completes with the bytes | S | `it::uploads::v24_pins` |
| V25 | A send replayed with the same key and body after its upload expired or was deleted | The original response with `deduplicated: true`; nothing re-checked, nothing sent again | S | `it::uploads::v25_replay_after_expiry` |
| V26 | A send refused after its copies were written (the step 7 transaction fails, or the thread lock times out) | The copies and sent copy are deleted; `use_count` unchanged; the upload stays `ready` and attachable | S | `it::uploads::v26_refused_send_cleanup` |
| V27 | Create repeated with the same `Idempotency-Key` | Same body: `201` with the same upload, a new `upload_url` and `Idempotent-Replayed: true`. Different body: `409 idempotency_conflict` | S | `it::uploads::v27_create_idempotency` |
| V28 | The 101st live upload of an identity, or live declared bytes above 524,288,000, including two creates racing for the last place | `422 upload_limit_reached` with `details.limit`, `used` and `max`; never more than the limit stored | S | `it::uploads::v28_limits` |
| V29 | Storage at or over the plan's `storage_gb` | Create gets `402 billing_limit` (`storage_gb`); a `PUT` of an existing upload is not gated; `exempt` and `disabled` billing skip the gate; upload bytes appear in the storage report | S | `it::uploads::v29_storage_gate` |
| V30 | Tenant suspended, partner suspended, identity paused, identity deleting, tenant erasing; a suspension that lands while a `PUT` streams | Create and `PUT` get `403 tenant_suspended`, `403 partner_suspended`, `409 identity_paused`; deleting or erasing: create `404 identity_not_found`, `PUT` `404 upload_not_found`; the in-flight `PUT`'s `CompleteUpload` still completes (the state is checked at the start), and the upload cannot be sent while the suspension lasts | S | `it::uploads::v30_suspension_and_pause` |
| V31 | A restore freeze or a `read_only` containment | Create, `PUT` and `DELETE` get `503 unavailable` with `details.reason` `frozen` or `read_only`; nothing written | S | `it::uploads::v31_freeze_and_read_only` |
| V32 | A key with neither `messages:send` nor `drafts:write`; an identity key naming another identity; a partner key naming another partner's tenant | `403 permission_denied` with `details.required` listing both and `any_of: true`; `404 upload_not_found` (or `identity_not_found`) for the scope cases, as for missing IDs | S | `it::uploads::scope_matrix` |
| V33 | Identity erasure or identity deletion, with and without legal holds | Every upload row and object of the identity is deleted; `uploads_deleted` in the receipt; held threads keep their messages' attachment copies | S | `it::uploads::erasure_scopes` |
| V34 | Message, thread or counterparty erasure of a message whose attachments came from uploads | The source uploads still present are deleted and counted; other messages' copies of the same upload are untouched | S | `it::uploads::erasure_scopes` |
| V35 | Tenant erasure; a `PUT` during it | Every upload is gone after `sweep_r2`; the `PUT` gets `404 upload_not_found` | S | `it::uploads::erasure_scopes` |
| V36 | A point-in-time restore brings back an upload row whose object was deleted | The first `PUT`, attach or alarm that finds the object missing sets `expired` (`object_missing`); a send gets `410 upload_expired`; nothing is sent with missing bytes; the `upload_object_missing` alert fires only outside a freeze | S | `it::uploads::v36_restore_object_missing` |
| V37 | An object written whose row never recorded it (crash before `CompleteUpload`), or a row deleted whose object delete failed | A retried `PUT` finds the stored object; the orphan sweep deletes an object no row names once it is 15 days old; the nightly backup job never copies `…/up/` keys | S | `it::uploads::v37_orphans_and_backup` |
| V38 | A test tenant | Uploads behave the same; a send attaching one reaches only simulator and loopback recipients (policy step 8) | S | `it::uploads::v38_test_tenant` |
| V39 | A dry run with upload attachments | Upload states and sizes validated and the link plan computed; no byte read, nothing copied, no pin left, `use_count` unchanged | S | `it::uploads::v39_dry_run` |
| V40 | `expires_in` longer than the tenant's `retention.message_days` or the identity's `retention.message_hours` | `expires_at` capped and shown in the response; the upload is deleted at that time; its tombstone answers `410` for 7 days, then `404` | S | `core::uploads::validate_create`, `it::uploads::expiry_alarm` |
| V41 | An inline base64 attachment with a quarantining risk | `422 attachment_risky` with `details.index` and `details.risk`; nothing stored (a change from earlier behaviour, where outbound attachments were not classified) | S | `it::uploads::inline_base64_classified` |
| V42 | A forward with `include_attachments: true` and `attachments` from uploads | Both sets attached; the total and transport limits count both | S | `it::uploads::v42_forward_attachments` |
| V43 | Ten counted failed `PUT`s | The upload becomes `expired` (`too_many_attempts`) and its object is deleted; `503` answers caused by the service are never counted | S | `core::uploads::state_machine`, `it::uploads::v43_attempt_limit` |
| V44 | A draft revision references an upload, and the upload is deleted or expires before the draft is sent | The revision's save copied the bytes; the draft sends with them; the send's attachment has the upload's ID in `upload_id` | S | `it::uploads::v44_draft_reference` |

## 18. Tests

| Test | Proves | Covers |
|---|---|---|
| `core::uploads::validate_create` | Every bound of create (size, digest pattern and lower-casing, filename sanitising and length, content type, `expires_in`), and the expiry cap by `message_days` and `message_hours` | [V1](../edge-cases.md), [V40](../edge-cases.md) |
| `core::uploads::token_purpose` | The upload token round-trips; expiry is `min(now + 3,600, expires_at)`; an `att` or `export` token is refused by the upload verifier and an `upload` token by the others; a flipped bit, a retired kid and a changed upload ID fail; comparison is constant-time (the shared verifier) | [V8](../edge-cases.md) |
| `core::uploads::state_machine` | Every transition of §2: claim from `pending` and from a lapsed lease, a second claim refused with the remaining lease, `ready` and `rejected` only with the claim, counted and uncounted failures, the tenth counted failure, expiry from each live state, tombstone purge after 7 days | §2, [V5](../edge-cases.md), [V43](../edge-cases.md) |
| `core::attach::v19_final_name_reclassified` | `classify_name` with a sniffed PDF: `report.exe`, `report.docm` and `report.zip` are risky, `Report.PDF` and `scan.pdf` are not; an octet-stream declared type is harmless | [V19](../edge-cases.md) |
| `core::compose::v23_link_plan_from_sizes` | `encoded_size` equals the length of the real base64 encoding with 76-character lines for sizes 0–4,000,000 (property test); the plan replaces the largest attachments first and fits the limit exactly at the boundary | [V23](../edge-cases.md) |
| `it::uploads::v_put_stream_contract` | Against workerd's R2, not a fake: a request body piped through `FixedLengthStream` into `put` with the `sha256` option stores an exact body and returns its size and checksum; a short body stores nothing and fails with an error the handler maps; a wrong digest fails with code `10037`; `head` returns the checksum. Written first in M33; if workerd differs from the page's assumptions, the page changes before the handler is built | [V2](../edge-cases.md), [V4](../edge-cases.md) |
| `it::uploads::v1_invalid_create` | Each invalid field gets `400 invalid_request` with its path; no row exists afterwards | [V1](../edge-cases.md) |
| `it::uploads::v2_partial_body` | A `PUT` that stops halfway (fault-injected client) leaves no object, counts one failure, returns the upload to `pending`; a full `PUT` then makes it `ready` | [V2](../edge-cases.md) |
| `it::uploads::v3_framing` | Chunked, wrong `Content-Length` and gzip `Content-Encoding` get `400 upload_size_mismatch` with the right `reason`; `failed_puts` unchanged; no R2 call made | [V3](../edge-cases.md) |
| `it::uploads::v4_checksum_mismatch` | Bytes with a different digest get `400 upload_checksum_mismatch`; no object; one failure counted | [V4](../edge-cases.md) |
| `it::uploads::v5_concurrent_puts` | Two `PUT`s started together: one `200`, one `409 upload_in_progress` with `Retry-After`; the object's digest is the declared one | [V5](../edge-cases.md) |
| `it::uploads::v6_put_after_ready` | A second `PUT` after `ready` gets `200` without the body being read (the fake request body records no read) and no second scanner call | [V6](../edge-cases.md) |
| `it::uploads::v7_put_after_expiry` | Clock past `expires_at`: `410 upload_expired` and the row is `expired`; after the 7-day purge: `404`; a token minted 61 minutes earlier: `404 upload_not_found` | [V7](../edge-cases.md) |
| `it::uploads::v9_crash_and_resume` | A `PUT` whose Worker is dropped after the R2 write: after 120 seconds the next `PUT` claims, the `head` check finds the object, no bytes are re-read, and the upload becomes `ready` | [V9](../edge-cases.md) |
| `it::uploads::v10_r2_errors` | Fault injection of R2 `10058` and a `500` on `put`: `503 unavailable`, `failed_puts` unchanged, the upload back to `pending` | [V10](../edge-cases.md) |
| `it::uploads::v11_classify_budget` | With the isolate budget held by another classification for 11 seconds, the `PUT` gets `503`; the retry classifies without streaming; `upload_classify_wait_total{result=timed_out}` | [V11](../edge-cases.md) |
| `it::uploads::v12_risky_rejected` | One fixture per quarantining risk is `rejected` with its `risk`, the object is gone, and attach gets `422 attachment_risky`; an encrypted PDF is `ready` with `risk = "encrypted_document"` and is sent | [V12](../edge-cases.md), [V13](../edge-cases.md) |
| `it::uploads::v14_scanner_verdicts` | With a fake scanner: `clean` → `ready` and `scan_status = "clean"`; `infected` → `rejected` (`infected`), object deleted; error, timeout, `500` and a non-JSON body → `ready` with `scan_status = "error"` and the counter; the request carries the two headers and the whole file | [V14](../edge-cases.md), [V15](../edge-cases.md) |
| `it::uploads::v16_cross_identity_attach` | A send from identity B naming A's upload, another tenant's upload and a random `upl_` ID gets three byte-identical `404 upload_not_found` bodies except `request_id` | [V16](../edge-cases.md) |
| `it::uploads::v17_attach_states` | Attach of `pending` and `uploading` uploads: `409 upload_not_ready`; of `expired`: `410`; of `rejected`: `422`; of `ready` past `expires_at` with the alarm suppressed: `410`; nothing stored in each case | [V17](../edge-cases.md), [V18](../edge-cases.md) |
| `it::uploads::v19_rename_refused` | A clean PDF upload attached as `report.exe` gets `422 attachment_risky`; attached as `claim.pdf` it is sent with that filename | [V19](../edge-cases.md) |
| `it::uploads::v20_attachment_item_shape` | Both, neither, and `upload_id` with `content_type` get `400` with the item's path; the same upload twice is sent as two attachments with distinct IDs and `use_count` 1 | [V20](../edge-cases.md), [V21](../edge-cases.md) |
| `it::uploads::v22_totals_and_links` | 26,214,401 bytes of attachments get `413` (`attachments_total`) under `refuse` and `link`; a 20 MiB upload under `link` is sent as a link whose download has the upload's digest; under `refuse` it gets `413` (`transport_limit`); a 1 MiB upload is embedded | [V22](../edge-cases.md), [V23](../edge-cases.md) |
| `it::uploads::v24_pins` | A send paused (test hook) between step 12 and step 7: `DELETE` gets `409 upload_in_use`; an expiry alarm firing then leaves the upload `ready` and re-arms; the send completes; after it, `DELETE` succeeds | [V24](../edge-cases.md) |
| `it::uploads::v25_replay_after_expiry` | Send accepted; upload deleted; the same request and key replayed: the original response with `deduplicated: true`, one message, one transport call | [V25](../edge-cases.md) |
| `it::uploads::v26_refused_send_cleanup` | With the step 7 transaction made to fail, no `out/{msg}/a/*` object remains, `use_count` is 0, and the upload is attachable by the next send | [V26](../edge-cases.md) |
| `it::uploads::v27_create_idempotency` | Same key and body: same upload ID, a different `upload_url`, `Idempotent-Replayed: true`; different body: `409 idempotency_conflict` | [V27](../edge-cases.md) |
| `it::uploads::v28_limits` | 100 live uploads then the 101st: `422` (`count`); declared bytes over 524,288,000: `422` (`bytes`); 20 creates racing for the last 3 places store exactly 3; tombstones do not count | [V28](../edge-cases.md) |
| `it::uploads::v29_storage_gate` | With `storage_gb` used = granted, create gets `402 billing_limit` (`storage_gb`) and a `PUT` of an earlier upload succeeds; `exempt` billing skips the gate; the hourly storage report includes a `ready` upload's size and excludes a deleted one | [V29](../edge-cases.md), §10 |
| `it::uploads::v30_suspension_and_pause` | Each state against create and `PUT` gives the listed code; a suspension set while a `PUT` streams lets that `PUT` complete and the send of it gets `403 tenant_suspended` | [V30](../edge-cases.md) |
| `it::uploads::v31_freeze_and_read_only` | Under a freeze and under `read_only`, create, `PUT` and `DELETE` get `503 unavailable` with the `reason`; no row or object changes | [V31](../edge-cases.md) |
| `it::uploads::scope_matrix` | The four routes and `PUT /v1/links/{token}` join `it::security::cross_tenant_matrix`; an identity key reaches only its own uploads; a key with only `drafts:write` and one with only `messages:send` both work; one with neither gets `403` with both listed | [V8](../edge-cases.md), [V32](../edge-cases.md), NFR-SEC-1 |
| `it::uploads::erasure_scopes` | Message, counterparty, identity (with and without a hold) and tenant erasure each delete the uploads they own, report `uploads_deleted`, and leave other messages' copies of a shared upload; probes find no `up/` object afterwards | [V33](../edge-cases.md)–[V35](../edge-cases.md), §9 |
| `it::uploads::v36_restore_object_missing` | A `ready` row with its object removed: a send gets `410 upload_expired` (`object_missing`), the row is `expired`, `uploads_expired_total{reason=object_missing}` is 1, and the alert evaluator fires `upload_object_missing` only when no freeze is set | [V36](../edge-cases.md) |
| `it::uploads::v37_orphans_and_backup` | An `up/` object with no row is kept at 14 days and deleted at 15 by the orphan sweep; the backup job's copy list never contains an `up/` key | [V37](../edge-cases.md) |
| `it::uploads::v38_test_tenant` | In a test tenant, an upload is created, `PUT` and sent to a simulator address; a send to an outside address gets `403 test_mode_recipient` as without uploads | [V38](../edge-cases.md) |
| `it::uploads::v39_dry_run` | `dry_run=true` with an expired upload gets `410`; with `ready` uploads it returns `would_send: true`, reads no R2 object (the fake counts zero reads), leaves `use_count` 0 and no pin | [V39](../edge-cases.md) |
| `it::uploads::expiry_alarm` | An unused upload is `expired` at `expires_at` and its object deleted; a failed delete is retried until it succeeds; the tombstone is purged after 7 days; the daily maintenance runs the sweep when the alarm was missed | [V40](../edge-cases.md), §5.3 |
| `it::uploads::inline_base64_classified` | A base64 PE file attached inline gets `422 attachment_risky` (`executable`); a base64 PDF is sent as before | [V41](../edge-cases.md) |
| `it::uploads::v42_forward_attachments` | A forward with the original's attachments and an upload sends both; with the total above the limit it gets `413` | [V42](../edge-cases.md) |
| `it::uploads::v43_attempt_limit` | Nine digest failures leave the upload `pending`; the tenth sets `expired` (`too_many_attempts`) and deletes nothing that was never stored; ten `503`s from R2 leave `failed_puts` at 0 | [V43](../edge-cases.md) |
| `it::uploads::v44_draft_reference` | A draft revision saved with an upload; the upload deleted; the draft sent: the message carries the bytes and `upload_id` | [V44](../edge-cases.md) |
| `it::uploads::lifecycle_and_reads` | Create, list (with `status`), get and delete answers; `upload_url` only in create and in `GET` of `pending` and `uploading`; `Message.attachments[].upload_id` set for copied attachments and `null` otherwise; the metrics of §14 are written | §2, §3, §14 |
| `it::mcp::upload_tools` | `mail_create_upload` and `mail_get_upload` are listed for keys with `messages:send` or `drafts:write` and not otherwise; they map to their routes and return the same errors; `mail_send` accepts an `upload_id` item and refuses an item with both shapes | §13 |
| `cli::uploads::create_and_attach` | Against recorded fakes: `pmail uploads create` sends the locally computed size and SHA-256, `PUT`s the file, retries a `409 upload_in_progress` after `Retry-After`, prints the ID; `pmail send --attach` inlines a 1 MiB file and uploads a 1 MiB + 1 byte file | §13 |
| `live::uploads::large_attachment_link` | On staging: a 20 MiB PDF uploaded and sent with `large_attachments: "link"` to a Gmail test mailbox arrives with a working link whose download has the upload's SHA-256; `upload_copy_ms` for it is recorded for the copy budget | [V23](../edge-cases.md), §4.4 |

## 19. Milestone

To add to the [build plan](../build-plan.md):

### M33 · Uploads (after M7, M9, M14, M15, M16 and M22)

(In `build-plan.md` this heading is level 2.)

**Files:** `crates/core/src/uploads.rs` (create validation, the state machine, the upload-token payload,
expiry arithmetic; pure), `crates/worker/src/uploads/{mod.rs, put.rs, classify.rs, copy.rs}`,
`handlers/uploads.rs` (the four `…/uploads` routes), `mailbox/uploads.rs` (the `uploads` table, its
operations and `alarm:uploads`), `crates/cli/src/commands/uploads.rs`. Mailbox schema: the `uploads` table
and `attachments.upload_id` (the next mailbox schema version). Hooks in other milestones' files, each
reviewed by that file's owner:

| File | Hook |
|---|---|
| `crates/platform/src/lib.rs`, `cf/glue.rs` (M4) | `BlobStore::put_stream(key, BodyStream, len, sha256, meta)`, `head_checksum(key) -> Option<(u64, Option<[u8; 32]>)>` and `copy(from, to, len, sha256)`; `RequestBody::stream()` for the handler; the fakes gain the same, with fault injection for `10037`, `10013` and `10058` |
| `router.rs`, `auth/routespec.rs` (M5) | `RouteSpec.any_permissions`; the four routes and `PUT /v1/links/{token}` |
| `handlers/links.rs` (M9) | `PUT` with purpose `upload`; `GET` refuses `upload` tokens |
| `mailbox/submit.rs`, `core/src/compose.rs` (M9) | Policy step 12 additions, the size plan from sizes, steps 4, 6 and 7 with uploads, pins; classification of inline base64 items |
| `core/src/attach.rs` (M7) | `classify_name` exposed for the final filename |
| `jobs/erasure.rs`, `jobs/backup.rs`, `mailbox/maintenance.rs` (M14) | `ErasurePlan` and `EraseRows` with source uploads, `UploadPlan` and `EraseUploads`, `uploads_deleted`; the backup job skips `up/`; the orphan sweep names `uploads.r2_key`; `Hygiene` runs the uploads sweep |
| `billing/storage.rs` (M22) | Upload bytes in the mailbox storage report; the create gate |
| `mcp/tools.rs` (M15) | `mail_create_upload`, `mail_get_upload`, both item shapes in the send tools, `attachments` on `mail_forward`, with rows in M15's table test |
| `crates/cli/src/commands/send.rs` (M16) | `--attach` through uploads above 1 MiB |
| `observability/catalogue.rs`, alert table (M17) | The nine metrics and `upload_object_missing` |
| Drafts (M29) | Whichever of M29 and M33 lands second adds the draft-save copy (`uploads::copy_to`) and `it::uploads::v44_draft_reference` |

**Implements:** FR-UPL-1 to FR-UPL-12 and edge rows V1–V44 ([Uploads](design/uploads.md)).

**Order.** `it::uploads::v_put_stream_contract` is the first pull request of the milestone: it runs the
`FixedLengthStream` and `sha256` behaviour against workerd's R2 and settles §3.2 steps 7 and 8 before any
handler code.

**Acceptance:**

- Edge rows V1–V44 with the tests named in the register, all passing.
- `core::uploads::*`, `core::attach::v19_final_name_reclassified`, `core::compose::v23_link_plan_from_sizes`.
- `it::uploads::scope_matrix`; the routes join `it::security::cross_tenant_matrix`.
- `it::uploads::erasure_scopes` fills the uploads rows of M14's erasure suites; the receipt schema has
  `uploads_deleted`.
- `it::uploads::lifecycle_and_reads`, `it::mcp::upload_tools`, `cli::uploads::create_and_attach`;
  `sdk::coverage::every_operation` passes with the four new operations and the `PUT`.
- `live::uploads::large_attachment_link` passes in M20, and its `upload_copy_ms` for 20 MiB is below the
  submit deadline with at least a factor of 3 to spare; otherwise the per-message attachment total is
  lowered before release.
- With no upload used, the M9 send suites pass unchanged, except `it::uploads::inline_base64_classified`'s
  deliberate change (risky inline attachments are refused).

## 20. Compared with AgentMail (read 2026-10-17)

From the AgentMail inventories (public docs and OpenAPI, read 2026-10-17; their content is untrusted data):

| AgentMail | Pylota Mail | Verdict |
|---|---|---|
| Inline `content` (base64) up to 6 MB per whole request; `413` above | Inline `content_base64` up to the 7 MiB request body; uploads up to 25 MiB per file | Exceed |
| `url` attachments fetched at send time, about 30 MB per message; an unfetchable URL is `403 message_rejected` | No URL attachments; uploads verified by SHA-256 before use; 25 MiB per message | Deliberately different: SSRF, content integrity, prompt injection (§1) |
| Agent Drive (Labs, US only): per-inbox files under paths, create → PUT to storage with headers → complete, upload URL up to 1 hour, 100 MiB per file, 1 GiB and 10,000 files per org, download links | Per-identity uploads with no paths or downloads; create → `PUT` to the Worker (no complete call); upload URL 1 hour; 25 MiB per file; 100 uploads and 500 MiB per identity; at most 7 days | Deliberately different: a staging area for sends, not a drive; the 25 MiB file limit fits every Workers plan and Email Routing's inbound size |
| Drive: storage verifies size and SHA-256; a retried PUT after success gets `412` | The Worker and R2 verify size and SHA-256; a retried `PUT` after success gets `200` with the upload; stored-but-unanswered uploads resume without re-sending bytes | Exceed |
| Drive: create without read can upload; another key's file at the same path gets `409` | Same-identity uploads only; another identity's or tenant's upload is indistinguishable from a missing one | Match (scope), exceed (no path collisions to leak) |
| Attachment risk handling is not described for sends (Agent Armor covers inbound) | Every upload and every inline attachment is sniffed and risk-classified; uploads also scanned when a scanner is configured; risky files refused | Exceed |
| MCP `upload_file` takes text or base64, about 7 MB through the hosted server | `mail_create_upload` returns an upload URL that a code tool `PUT`s to, so no file passes through model output; inline base64 still allowed within 7 MiB | Exceed |
| Drive files deleted with the inbox | Uploads deleted by every erasure scope with receipt counts, by expiry, and excluded from backups | Exceed |

## 21. Integration checklist

Edits to shared files once PR #5 has merged, in this order. Text in blocks is meant to be pasted.

**1. `project/prd.md`.**

- § 6: add the group "Uploads" with FR-UPL-1 to FR-UPL-12 exactly as in [§16](#16-functional-requirements).
- FR-OUT-10 (message size): append "Attachments may also come from uploads (FR-UPL-4); the transport
  limit and `large_attachments` apply to them alike, and a message's attachments total at most
  26,214,400 decoded bytes (FR-UPL-5)."
- Scope table, v1.0 row to add:

  ```markdown
  | Uploads: stage a file once per identity (≤ 25 MiB, SHA-256 verified, classified and scanned) and attach it by `upload_id` | v1.0 | [Uploads](design/uploads.md) |
  ```

- Non-goals, rows to add:

  ```markdown
  - **Attachments fetched from URLs.** Pylota Mail never fetches a URL to build an attachment (SSRF, content integrity, prompt injection; [Uploads § 1](design/uploads.md#1-what-uploads-are-and-are-not)). Upload the file instead.
  - **A file drive.** Uploads have no paths, folders, downloads or sharing and live at most 7 days; long-lived files belong in the integrator's own storage.
  ```

**2. `reference/api.md`.**

- Basics › IDs: add `upl_` to the prefix list.
- Permissions table: change the `messages:send` row to
  `| messages:send | Send, reply, reply-all, forward, cancel; create, read and delete uploads (or with drafts:write) |`.
  The `drafts:write` row (Drafts page) gains "; create, read and delete uploads (or with `messages:send`)".
  After the table, add: "The upload routes take either `messages:send` or `drafts:write`: a key needs one
  of them, not both."
- Route index: add the five rows

  ```markdown
  | `POST /v1/identities/{identity_id}/uploads` | Create an upload | `messages:send` or `drafts:write` | [Uploads](#uploads) |
  | `GET /v1/identities/{identity_id}/uploads` | List uploads | `messages:send` or `drafts:write` | [Uploads](#uploads) |
  | `GET /v1/identities/{identity_id}/uploads/{upload_id}` | Get an upload (with a fresh upload URL while it waits for bytes) | `messages:send` or `drafts:write` | [Uploads](#uploads) |
  | `DELETE /v1/identities/{identity_id}/uploads/{upload_id}` | Delete an upload | `messages:send` or `drafts:write` | [Uploads](#uploads) |
  | `PUT /v1/links/{token}` | Send an upload's bytes (link signature) | Yes | [Uploads](#put-v1linkstoken) |
  ```

- New section "Uploads", after "Sending": §2 (the object and its table), §3.1–§3.4 of this page
  (request and response JSON, the rules as numbered lists, the errors), and this paragraph:

  ```markdown
  An upload stages one file for one identity: create it with the file's name, type, size and SHA-256,
  `PUT` the bytes to its `upload_url`, then attach it by `upload_id` to sends, replies, reply-alls,
  forwards and drafts of the same identity until `expires_at`. Each message copies the bytes, so an
  upload can be reused and its expiry never changes a sent message. There is no download route. Pylota
  Mail never fetches attachments from URLs.
  ```

- Sending › `POST /v1/identities/{identity_id}/messages`, replace the attachments bullet with:

  ```markdown
  - Attachments: each item is either inline (`filename`, `content_type`, `content_base64`) or an upload
    (`upload_id`, optional `filename`), with `disposition` (`attachment` or `inline`) and `content_id`
    (for inline). An upload must be `ready` and belong to the sending identity (`404 upload_not_found`,
    `409 upload_not_ready`, `410 upload_expired`). Every attachment is classified like inbound mail, and
    one with a quarantining risk (also after a rename) gets `422 attachment_risky`. Attachments total at
    most 26,214,400 decoded bytes; the encoded message must fit the transport limit (5 MiB with
    Cloudflare) or the request fails with `413 message_too_large` (`details.reason` `attachments_total` or
    `transport_limit`). When the tenant enables `large_attachments: "link"`, oversized attachments become
    expiring signed links instead.
  ```

- `…/forward`: add `"attachments": [ { "upload_id": "upl_01JAC3Q7W9X2M5N6P8R0T1Y4ZB" } ]` to the example
  and "`attachments` adds files, inline or from uploads, as for send; `include_attachments` keeps the
  original's." below it.
- Message object › `attachments[]`: add `"upload_id": "upl_…" | null` with "the upload the attachment was
  copied from; `null` for inline, inbound and forwarded attachments. It stays after the upload is gone."
- Signed links: add `### PUT /v1/links/{token}` with §3.2's request, response, the purpose rule and the
  error list; in `GET /v1/links/{token}` add "An upload token gets `404 upload_not_found`."
- Erasure request object › receipt: add `"uploads_deleted": 3` with "upload rows deleted".

**3. `reference/openapi.yaml`.**

Vendor extensions table, new row:

```yaml
    | `x-required-permission-any` | operation | Array of permissions of which the key must hold at least one. Used with `x-required-permission: []` |
```

`info.description` › IDs: add `upl_` to the list. Tags: add

```yaml
  - name: Uploads
    description: >-
      Files staged by an identity and attached to its sends by `upload_id`. Create, `PUT` the bytes to the
      signed `upload_url`, attach. `messages:send` or `drafts:write`.
```

Paths:

```yaml
  /identities/{identity_id}/uploads:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: createUpload
      tags: [Uploads]
      summary: Create an upload
      description: >-
        Declares a file (name, type, size, SHA-256) and returns an upload with a signed `upload_url`, valid
        for at most one hour, to `PUT` the bytes to. The upload becomes `ready` when the bytes match and pass
        classification (and the scanner, when configured), and can then be attached by `upload_id` to sends
        of the same identity until `expires_at`. A replay re-reads the upload and mints a new `upload_url`.
      x-required-permission: []
      x-required-permission-any: ['messages:send', 'drafts:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: An identity key creates uploads only for its own identity.
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/UploadCreateRequest'}
            example:
              filename: FNOL-7781.pdf
              content_type: application/pdf
              size: 9437184
              sha256: 2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae
              expires_in: 86400
      responses:
        '201':
          description: The upload, `pending`, with `upload_url`.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Upload'}
        '400':
          description: A field is out of bounds, or the idempotency key is invalid.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '402': {$ref: '#/components/responses/BillingLimit'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: The identity is paused (`identity_paused`), or the idempotency key is in use with another body.
          x-error-codes: [identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            The identity already has 100 live uploads or 524,288,000 declared bytes of them
            (`upload_limit_reached`, `details.limit` = `count` or `bytes`, `details.used`, `details.max`).
          x-error-codes: [upload_limit_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listUploads
      tags: [Uploads]
      summary: List uploads
      description: >-
        The identity's uploads, newest first, including `rejected` and `expired` tombstones while they
        exist (7 days). Never carries `upload_url`.
      x-required-permission: []
      x-required-permission-any: ['messages:send', 'drafts:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
        - name: status
          in: query
          schema: {$ref: '#/components/schemas/UploadStatus'}
      responses:
        '200':
          description: A page of uploads.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema:
                type: object
                required: [data, next_cursor]
                properties:
                  data:
                    type: array
                    items: {$ref: '#/components/schemas/Upload'}
                  next_cursor: {type: [string, 'null']}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/uploads/{upload_id}:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/UploadIdPath'
    get:
      operationId: getUpload
      tags: [Uploads]
      summary: Get an upload
      description: The upload. While it is `pending` or `uploading`, the response carries a newly minted `upload_url`.
      x-required-permission: []
      x-required-permission-any: ['messages:send', 'drafts:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: The upload.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Upload'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404':
          description: No such upload in scope (`upload_not_found`), or no such identity.
          x-error-codes: [upload_not_found, identity_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: deleteUpload
      tags: [Uploads]
      summary: Delete an upload
      description: >-
        Deletes the upload and its bytes in any state. Messages and drafts that copied it keep their copies.
        A send that is attaching it at that moment makes the delete answer `409 upload_in_use`.
      x-required-permission: []
      x-required-permission-any: ['messages:send', 'drafts:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '204':
          description: Deleted.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404':
          description: No such upload in scope.
          x-error-codes: [upload_not_found, identity_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: A send is attaching the upload right now (`upload_in_use`, retryable, `Retry-After: 5`).
          x-error-codes: [upload_in_use]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            Retry-After: {$ref: '#/components/headers/Retry-After'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
```

Under the existing `/links/{token}` path item, next to `get`:

```yaml
    put:
      operationId: putUploadBytes
      tags: [Uploads]
      summary: Send an upload's bytes
      description: >-
        No API key. The `upload_url` of an upload. `Content-Length` must equal the declared size; the bytes
        are streamed into storage and checked against the declared SHA-256, then classified (and scanned,
        when a scanner is configured). A repeated `PUT` of a `ready` upload answers `200` without reading
        the body. A bad, expired or other-purpose token returns `404 upload_not_found`.
      security: []
      x-rate-limit-buckets: []
      x-permission-notes: Counted against `RL_TENANT` of the token's tenant.
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/octet-stream:
            schema:
              type: string
              contentMediaType: application/octet-stream
              maxLength: 26214400
      responses:
        '200':
          description: The upload, `ready`.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Upload'}
        '400':
          description: >-
            `Content-Length` missing or not the declared size, a `Content-Encoding`, or a body that ended early
            (`upload_size_mismatch`, `details.reason` = `content_length`, `encoding` or `incomplete`); bytes
            whose SHA-256 is not the declared one (`upload_checksum_mismatch`).
          x-error-codes: [upload_size_mismatch, upload_checksum_mismatch]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: The tenant or its partner is suspended.
          x-error-codes: [tenant_suspended, partner_suspended]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404':
          description: The token is bad, expired or of another purpose, or the upload, identity or tenant is gone.
          x-error-codes: [upload_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: Another `PUT` holds the upload (`upload_in_progress`, `Retry-After`), or the identity is paused.
          x-error-codes: [upload_in_progress, identity_paused]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            Retry-After: {$ref: '#/components/headers/Retry-After'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '410':
          description: The upload expired (`upload_expired`, `details.reason`).
          x-error-codes: [upload_expired]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: The bytes carry a quarantining risk or the scanner found malware (`attachment_risky`, `details.risk` or `details.reason = "infected"`).
          x-error-codes: [attachment_risky]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
```

Components:

```yaml
  parameters:
    UploadIdPath:
      name: upload_id
      in: path
      required: true
      schema: {$ref: '#/components/schemas/UploadId'}

  schemas:
    UploadId:
      type: string
      pattern: '^upl_[0-9A-HJKMNP-TV-Z]{26}$'
    UploadStatus:
      type: string
      enum: [pending, uploading, ready, rejected, expired]
    UploadCreateRequest:
      type: object
      additionalProperties: false
      required: [filename, content_type, size, sha256]
      properties:
        filename: {type: string, minLength: 1, maxLength: 255}
        content_type: {type: string, maxLength: 127, examples: [application/pdf]}
        size: {type: integer, minimum: 1, maximum: 26214400, description: Bytes.}
        sha256: {type: string, pattern: '^[0-9A-Fa-f]{64}$', description: SHA-256 of the bytes, hexadecimal.}
        expires_in: {type: integer, minimum: 900, maximum: 604800, default: 86400, description: Seconds; capped by the identity's mail retention.}
    Upload:
      type: object
      required: [id, identity_id, filename, content_type, size, sha256, status, status_reason, sniffed_type,
                 risk, scan_status, use_count, last_used_at, created_at, expires_at, upload_url, upload_url_expires_at]
      properties:
        id: {$ref: '#/components/schemas/UploadId'}
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        filename: {type: string, description: Sanitised. Untrusted content.}
        content_type: {type: string}
        size: {type: integer, minimum: 1, maximum: 26214400}
        sha256: {type: string, pattern: '^[0-9a-f]{64}$'}
        status: {$ref: '#/components/schemas/UploadStatus'}
        status_reason:
          type: [string, 'null']
          enum: [risk, infected, expired, too_many_attempts, object_missing, null]
        sniffed_type: {type: [string, 'null']}
        risk:
          oneOf:
            - $ref: '#/components/schemas/AttachmentRisk'
            - type: 'null'
        scan_status: {type: string, enum: [skipped, clean, infected, error]}
        use_count: {type: integer, minimum: 0}
        last_used_at: {type: [string, 'null'], format: date-time}
        created_at: {type: string, format: date-time}
        expires_at: {type: string, format: date-time}
        upload_url:
          type: [string, 'null']
          format: uri
          description: Only in the create response and in getUpload while `pending` or `uploading`. A bearer URL; valid until `upload_url_expires_at`.
        upload_url_expires_at: {type: [string, 'null'], format: date-time}
    AttachmentUploadInput:
      type: object
      additionalProperties: false
      required: [upload_id]
      properties:
        upload_id: {$ref: '#/components/schemas/UploadId'}
        filename: {type: string, minLength: 1, maxLength: 255, description: Defaults to the upload's filename.}
        disposition: {type: string, enum: [attachment, inline], default: attachment}
        content_id: {type: string, description: The Content-ID for an `inline` attachment.}
    SendAttachment:
      oneOf:
        - $ref: '#/components/schemas/AttachmentInput'
        - $ref: '#/components/schemas/AttachmentUploadInput'
```

- `AttachmentInput`: add `additionalProperties: false` so that the two shapes cannot both match.
- In the send, reply, reply-all and forward request schemas, `attachments.items` becomes
  `{$ref: '#/components/schemas/SendAttachment'}`; the forward request gains
  `attachments: {type: array, maxItems: 32, items: {$ref: '#/components/schemas/SendAttachment'}}`.
- `Attachment` gains `upload_id: {oneOf: [{$ref: '#/components/schemas/UploadId'}, {type: 'null'}]}`
  (and in `required`).
- `ErasureReceipt` gains `uploads_deleted: {type: integer, minimum: 0}` (and in `required`).
- `x-error-codes` of the send operations: `404` adds `upload_not_found`; `409` adds `upload_not_ready`;
  a new `410` with `[upload_expired]`; `422` adds `attachment_risky`; the `413 message_too_large`
  description adds `details.reason` (`attachments_total` or `transport_limit`).

**4. `reference/errors.md`.**

Validation:

```markdown
| 400 | `upload_size_mismatch` | no | An upload `PUT` whose `Content-Length` is missing or not the declared size (`details.reason = "content_length"`), that has a `Content-Encoding` (`"encoding"`), or whose body ended early (`"incomplete"`). `details.expected` and `details.received` |
| 400 | `upload_checksum_mismatch` | no | The bytes of an upload `PUT` do not have the declared SHA-256. Nothing was stored. `details.expected` |
```

The `413 message_too_large` row becomes: "The composed message exceeds the transport limit (5 MiB with
Cloudflare; `details.reason = "transport_limit"`), or its attachments total more than 26,214,400 decoded
bytes (`"attachments_total"`, whatever `large_attachments` says)".

Resources and state: add `upload_not_found` to the `404` list, and

```markdown
| 409 | `upload_not_ready` | yes | A send attaches an upload that is still `pending` or `uploading`. `details.index` and `details.status`. Finish the `PUT` first |
| 409 | `upload_in_progress` | yes | Another `PUT` of the upload is running. Retry after `Retry-After` |
| 409 | `upload_in_use` | yes | `DELETE` of an upload a send is attaching at that moment. Retry after `Retry-After` |
| 410 | `upload_expired` | no | The upload expired (`details.reason`: `expired`, `too_many_attempts` or `object_missing`). Upload the file again |
```

Policy and limits:

```markdown
| 422 | `upload_limit_reached` | no | The identity already has 100 live uploads, or 524,288,000 declared bytes of them. `details.limit` (`count` or `bytes`), `details.used`, `details.max`. Delete unused uploads or wait for them to expire |
| 422 | `attachment_risky` | no | An attachment (an upload, or an inline attachment of a send) has a quarantining risk (`details.risk`: `executable`, `macro`, `archive_bomb`, `encrypted_archive`, `type_mismatch`), including after a rename, or the scanner found malware (`details.reason = "infected"`). `details.index` names the attachment of a send |
```

**5. `reference/events.md`.** No change: uploads emit no event (§11).

**6. `reference/limits.md`.** New section after "Service sign-up ledger":

```markdown
## Uploads

From [Uploads](../project/design/uploads.md).

| Limit | Value | Source | What happens |
|---|---|---|---|
| Upload size | 1–26,214,400 bytes (25 MiB) | Pylota Mail; below the Workers request-body limit of every account plan (100 MB Free and Pro, 200 MB Business; [Workers limits](https://developers.cloudflare.com/workers/platform/limits/), read 2026-10-17) | `400 invalid_request` at create; a `PUT` of another length gets `400 upload_size_mismatch` |
| Live uploads per identity | 100, and 524,288,000 declared bytes | Pylota Mail | `422 upload_limit_reached` |
| Upload lifetime | 900–604,800 seconds, default 86,400; never past the identity's mail retention | Pylota Mail | Deleted at `expires_at`; `410 upload_expired` for 7 days, then `404` |
| Upload URL | 3,600 seconds, never past `expires_at` | Pylota Mail | `404 upload_not_found`; `GET` the upload for a new URL |
| Failed `PUT`s | 10 caused by the client | Pylota Mail | `expired` (`too_many_attempts`) |
| Attachments per message, decoded | 26,214,400 bytes in total | Pylota Mail | `413 message_too_large` (`attachments_total`) |
```

Mail table, "Attachments per send" row: append "; each inline or from an upload".

**7. `reference/configuration.md`.** No new policy field. `PM_SCANNER_URL` row: "Optional malware scanner
endpoint for inbound attachments and uploads (see [Inbound](…#attachment-safety) and
[Uploads](../project/design/uploads.md#43-classification-and-the-scanner))". `large_attachments` row:
append "Applies to attachments from uploads too."

**8. `project/design/data-model.md`.**

- Conventions › ID prefixes: add `upl_` (uploads, mailbox).
- § 2: the `uploads` DDL and the `attachments.upload_id` statement of [§5.1](#51-mailbox-table); the
  `meta` key list gains `alarm:uploads   earliest upload expiry, tombstone purge or object-delete retry
  (Uploads § 5.3)`.
- § 4: the `up/{upl}` row of §5.1; the `out/{msg}/a/{att}` row's content becomes "Outbound attachment
  bytes (linked attachments, copies for `GET …/attachments/{id}`, and copies of uploads)"; the backup
  paragraph gains "except keys under `…/up/`".

**9. `project/edge-cases.md`.** New section "V · Uploads" with rows V1–V44 of [§17](#17-edge-cases); the
header's counts gain 44 rows and section V (all owned by `S`).

**10. `project/build-plan.md`.** The milestone of [§19](#19-milestone) as `## M33 · Uploads (after M7, M9,
M14, M15, M16 and M22)`; in the second dependency graph:

```text
M7, M9, M14, M15, M16, M22 ─▶ M33 uploads ─▶ M20
   (M33 adds two tools to M15's table, commands to M16's CLI, and a draft-save hook shared with M29)
```

**11. `project/design/testing.md`.** Fakes: `BlobStore` fake gains `put_stream`, `head_checksum` and
`copy`, with fault injection of R2 codes `10013`, `10037` and `10058`. Attack suite: the four upload
routes and `PUT /v1/links/{token}` join `it::security::cross_tenant_matrix`. The real-runtime list gains
`it::uploads::v_put_stream_contract` (workerd's R2, never a fake).

**12. `project/design/security.md`.**

- § 5.1: `RouteSpec` gains
  `pub any_permissions: &'static [Permission], // at least one required when not empty (Uploads)`.
- § 5.2 step 2: after the first sentence add "When `RouteSpec.any_permissions` is not empty, the key must
  also hold at least one of them; otherwise `403 permission_denied` with `details.required` listing them
  and `details.any_of: true`."
- § 7.3: the payload alternatives gain
  `| "l1:{kid}:upload:{tenant_id}:{identity_id}:{upload_id}:{expires_unix_s}"   -- upload: PUT only, at most 1 hour ahead`,
  and the bullet list gains: "An `upload` token is accepted only by `PUT /v1/links/{token}`, and the
  `att` and `export` tokens only by `GET`; a token used with the other method answers as a bad one."
- § 9.3: add "No request field ever makes the Worker fetch a URL; attachments come inline or from
  uploads ([Uploads § 1](uploads.md#1-what-uploads-are-and-are-not))."

**13. `project/design/outbound.md`.** Policy step 12's check column becomes "…; ≤ 32 attachments, each
inline with valid base64 or an upload of this identity that is `ready` ([Uploads § 4.1](uploads.md#41-policy-step-12-extended));
every attachment classified; attachments total ≤ 26,214,400 decoded bytes; `content_id` on inline parts;
labels and metadata within limits", and its error column gains `404 upload_not_found`,
`409 upload_not_ready`, `410 upload_expired`, `422 attachment_risky`, `413 message_too_large`
(`attachments_total`). "Attachments and size" gains a first bullet: "An attachment is inline
(`content_base64`) or an upload (`upload_id`, [Uploads](uploads.md)); step 17 plans embedding and links
from sizes alone, and only embedded uploads are read before composition." Reservation steps 4, 6 and 7
gain the upload sentences of [§4.4](#44-steps-6-and-7-of-the-reservation).

**14. `project/design/inbound.md`.** Malware scanner hook: append "The same hook scans uploads, whole, up to
26,214,400 bytes ([Uploads § 4.3](uploads.md#43-classification-and-the-scanner))." Orphan objects: add
`uploads.r2_key` to the keys that keep an object.

**15. `project/design/privacy.md`.** § 2 inventory row
`| Uploads | Mailbox uploads rows and R2 t/{ten}/i/{idn}/up/ | Files an agent staged to send, filenames | At most 7 days, capped by retention; tombstones 7 days without bytes | Every scope (Uploads § 9) |`;
§ 4.1 the `UploadPlan` and `EraseUploads` rows and the `ErasurePlan` and `EraseRows` additions; § 6.3,
§ 6.4 and § 6.5 the sentences of [§9](#9-privacy); § 5.4 "except keys under `…/up/`"; § 10 the
`uploads_deleted` field and row "`uploads_deleted` | `uploads` rows deleted (including by `WipeAll`, from
`CountAll`)"; § 11 the row "Unsent uploads | until `expires_at` (at most 7 days) | Not exported; not in the
backup bucket".

**16. `project/design/billing.md`.** Storage: "…`attachments.size`, the `size` of `uploads` whose object
is present or being deleted, and its own SQLite size…"; Hold: "Create of an upload is refused with
`402 billing_limit` (`storage_gb`) while storage is at or over its allowance."

**17. `project/design/mcp.md` and `reference/mcp.md`.** Tool permission table rows

```markdown
| `mail_create_upload` | `messages:send` or `drafts:write` | – |
| `mail_get_upload` | `messages:send` or `drafts:write` | – |
```

tool map rows

```markdown
| `mail_create_upload` | `POST /v1/identities/{id}/uploads` (optional `idempotency_key` → `Idempotency-Key`) | – (`RL_API` only) |
| `mail_get_upload` | `GET /v1/identities/{id}/uploads/{upload_id}` | – (`RL_API` only) |
```

annotations (`mail_create_upload`: `false`, `false`, `false`, `false`; `mail_get_upload`: read-only
defaults); definitions:

```json
{ "name": "mail_create_upload", "title": "Stage a file to attach",
  "description": "Create an upload for a file you will attach to an email. Give the exact size in bytes and the SHA-256 of the file. Then send the file bytes with an HTTP PUT to upload_url from a tool that can read the file (for example: curl -sS -T FILE -H 'Content-Type: application/octet-stream' \"$UPLOAD_URL\"), and pass upload_id in attachments of mail_send, mail_reply or mail_forward. The URL works for one hour and only for these exact bytes; the upload expires at expires_at.",
  "inputSchema": { "type": "object", "additionalProperties": false, "required": ["filename", "content_type", "size", "sha256"], "properties": {
      "identity": { "type": "string", "maxLength": 254 },
      "filename": { "type": "string", "minLength": 1, "maxLength": 255 },
      "content_type": { "type": "string", "maxLength": 127 },
      "size": { "type": "integer", "minimum": 1, "maximum": 26214400 },
      "sha256": { "type": "string", "pattern": "^[0-9A-Fa-f]{64}$" },
      "expires_in": { "type": "integer", "minimum": 900, "maximum": 604800 },
      "idempotency_key": { "type": "string", "minLength": 1, "maxLength": 255, "pattern": "^[\\x20-\\x7E]{1,255}$" } } } }
{ "name": "mail_get_upload", "title": "Check a staged file",
  "description": "Read an upload's status (pending, uploading, ready, rejected, expired). While it waits for bytes, the result has a fresh upload_url. Only ready uploads can be attached.",
  "inputSchema": { "type": "object", "additionalProperties": false, "required": ["upload_id"], "properties": {
      "identity": { "type": "string", "maxLength": 254 },
      "upload_id": { "type": "string", "pattern": "^upl_[0-9A-HJKMNP-TV-Z]{26}$" } } } }
```

and in `mail_send` (and through it `mail_reply`) the attachment item becomes

```json
"items": { "oneOf": [
  { "type": "object", "additionalProperties": false, "required": ["filename", "content_type", "content_base64"], "properties": {
      "filename": { "type": "string", "maxLength": 255 }, "content_type": { "type": "string", "maxLength": 127 },
      "content_base64": { "type": "string" },
      "disposition": { "type": "string", "enum": ["attachment", "inline"], "default": "attachment" },
      "content_id": { "type": "string", "maxLength": 255 } } },
  { "type": "object", "additionalProperties": false, "required": ["upload_id"], "properties": {
      "upload_id": { "type": "string", "pattern": "^upl_[0-9A-HJKMNP-TV-Z]{26}$" },
      "filename": { "type": "string", "maxLength": 255 },
      "disposition": { "type": "string", "enum": ["attachment", "inline"], "default": "attachment" },
      "content_id": { "type": "string", "maxLength": 255 } } } ] }
```

`mail_forward` gains `"attachments": { "type": "array", "maxItems": 10, "items": { "…": "as mail_send" } }`.
The `mail_send` description gains: "For a file larger than a few hundred kilobytes, use mail_create_upload
and attach it by upload_id instead of base64."

**18. `reference/cli.md`.** Commands at a glance gains the four `pmail uploads` lines of [§13](#13-mcp-and-cli);
`pmail send` › `--attach FILE`: "Repeatable. Files up to 1 MiB are sent inline; larger files are uploaded
first (`pmail uploads create`) and attached by ID. At most 25 MiB per file and 25 MiB per message."

**19. `project/design/console.md`.** No change (§12).

**20. `project/design/observability.md`.** § 3.2: the nine rows of [§14](#14-observability); § 5.3:

```markdown
| `upload_object_missing` | B | `uploads_expired_total{reason=object_missing}` > 0 while no restore freeze is in effect: an upload's object disappeared without a restore ([V36](../edge-cases.md)) | ticket | Compare the mailbox's `uploads` rows with `BLOBS.list` of the identity's `up/` prefix and the orphan sweep's log |
```

§ 6 Outbound and delivery dashboard: "; uploads by `PUT` result, `upload_put_ms` and `upload_copy_ms`
p50/p95/p99, inline against linked attachments".
