# Agent credential vault

Binding for implementation. The vault keeps the credentials an agent needs to act at third-party services
(logins with their TOTP seeds, API keys and tokens, and browser session bundles) inside Pylota Identity,
beside the agent's identity and its entry in the [service sign-up ledger](service-accounts.md). No secret
value ever reaches a model, an MCP result, a log, an event or an API response. An agent holds item handles
and asks for **grants**; a person approves; the integrator's orchestrator asks for a **use**; and a
registered **consumer** (for Pylota, the browser fleet or its egress proxy) redeems a one-use **ticket** that
is bound to the item's exact origin, the run, the agent and a short expiry, and receives the values sealed to
its own key. Pylota Identity Cloud holds the keys that decrypt the vault, and this page says so: it is a
server-custody vault, not a zero-knowledge one ([§1.2](#12-the-trust-model)). The vault replaces Pylota's
`browser_sealed_sessions` ([§22](#22-pylota-agentic-os)).
Decided in [ADR 0021](../adr/0021-agent-vault.md); risk tier T2.

| | |
|---|---|
| Requirements | FR-VLT-1 to FR-VLT-25 (this page), FR-IDN-10 (the ledger gains `login_domains` and a vault link), FR-IN-5 and FR-IN-11 (quarantine rule 4b and `wait`), FR-TEN-4 (seven `vault.*` policy fields), FR-CON-6 (two new decisions reserved for people), FR-PRV-3 (erasure receipts), FR-KEY-1 (six permissions) |
| Edge cases | [U1](../edge-cases.md)–[U72](../edge-cases.md) (section `U · Vault`) |
| Code | `crates/core/src/vault/{mod.rs, origin.rs, item.rs, policy.rs, digest.rs, ticket.rs, totp.rs, cookies.rs, audit.rs, refuse.rs, consumer.rs}` (origin canonicalisation and matching, item validation and refused kinds, the approval decision, the use digest, the ticket token, RFC 6238, session filtering, the audit row hash, the redemption checks; pure), `crates/core/src/crypto/{hpke.rs, pv1.rs}` (the HPKE suite and the `pv1` field envelope; pure, nonces and ephemeral keys passed in), `crates/api-types/src/vault.rs`, `crates/worker/src/vault/{mod.rs, keys.rs, items.rs, grants.rs, uses.rs, tickets.rs, consumers.rs, redeem.rs, codes.rs, sessions.rs, audit.rs, erase.rs}`, `handlers/vault.rs` (API-key routes), `handlers/vault_consumer.rs` (consumer routes), `auth/consumer.rs` (signed-request verification), `auth/people.rs` (two new `Decision` arms), `mailbox/ingest.rs` (quarantine rule 4b), `mailbox/codes.rs` (`VaultTakeCode`), `handlers/wait.rs` (the strict-mode refusal), `jobs/erasure.rs` and `jobs/retention.rs` (the vault steps), `console/pages/vault.rs`, `mcp/tools.rs` (six tools), `crates/cli/src/commands/vault.rs` |
| Tables | D1 `vault_keys`, `vault_custody_slots`, `vault_items`, `vault_item_versions`, `vault_grants`, `vault_uses`, `vault_consumers`, `vault_consumer_nonces`, `vault_audit`, `vault_audit_anchors`; new columns `service_accounts.login_domains_json`; mailbox `verifications.vault_item_id` and `verifications.vault_use_id`, the quarantine reason `vault_code`; R2 `t/{tenant_id}/vault/{item_id}/{version}` ([Data model](data-model.md)) |
| Contracts | The `/v1/…/vault/…` routes and the consumer routes `/v1/vault/consumer/{self, redeem, report, write-back}` ([REST API › Vault](../../reference/api.md#vault)); the permissions `vault:read`, `vault:write`, `vault:request`, `vault:use`, `vault:approve`, `vault:admin`; the consumer signature scheme `pylota-identity-vault-consumer`; the header `X-Pylota-Identity-Vault-Unlock`; 22 `vault.*` event types ([Webhook events › Vault](../../reference/events.md#vault)); 21 error codes ([Errors](../../reference/errors.md)); the policy fields `vault.allowed`, `vault.key_approval`, `vault.min_approval`, `vault.codes_into_fills_only`, `vault.max_items`, `vault.session_max_ttl_days`, `vault.approval_deadline_seconds`; the MCP tools `vault_list_items`, `vault_get_item`, `vault_request_grant`, `vault_list_grants`, `vault_get_use` and `vault_sign`, and a `vault_login` argument on `mail_request_account`; `idvy vault …` ([CLI](../../reference/cli.md#vault)) |
| Limits | [Limits › Vault](../../reference/limits.md#vault) and [§19](#19-billing-and-limits) |
| Built in | M42 (core, keys, items, audit, erasure), M43 (grants, uses, approval, consumers, tickets), M44 (one-time codes and sessions), M45 (rotation, signing, reveal, custody, tooling and the Pylota cut-over) ([Build plan](../build-plan.md)) |

## 1. What the vault is, and what it is not

### 1.1 Scope

| In v1 | Deferred | Refused |
|---|---|---|
| Items of three kinds an agent can use: `login` (username, password, optional TOTP seed, optional recovery codes, exact URLs), `api_key` (a token or secret with the hosts it may be used on) and `session` (cookies and web storage for exact origins, with a time to live). One internal kind, `managed_secret`, that only the deployment itself writes, for the phone design's port-out PIN ([§13.2](#132-managed-secrets)) | Credentialed fetch through Pylota Identity ([§1.4](#14-credentialed-fetch-is-deferred)); share links to people; automated password change at sites beyond `rotate_fill`; breach checks of stored passwords; becoming a 1Password Agentic Autofill partner; SSH keys and documents | Payment cards and bank account details; passkeys and any WebAuthn emulation; CAPTCHA solving, and storing bot-challenge clearance cookies; plaintext export of any value to an agent, a model, an MCP client or an API key; self-approval; a claim of zero knowledge or end-to-end encryption |
| Use without reveal: fill tickets for a browser consumer, TOTP codes and one-time codes from the identity's own mailbox or phone number delivered into a fill, session loading, secret injection by an executor consumer, and (P1) signing | | |
| Grants, per-use approval by a person, an append-only audit, rotation, session expiry with alerts, erasure with receipts | | |

The refusals are reasoned in [ADR 0021](../adr/0021-agent-vault.md) and enforced in [§3.6](#36-refused-items-and-values).

### 1.2 The trust model

The vault must sign in, compute codes and seal packets on its own, so the Worker must be able to decrypt.
These are the facts the honest statement rests on:

- Workers run customer code in V8 isolates, many to a process, and Cloudflare's own page describes
  isolation, Spectre mitigations and dynamic process isolation, not hardware attestation
  ([Workers security model](https://developers.cloudflare.com/workers/reference/security-model/), updated
  2026-10-18, read 2026-10-24).
- To a Worker, a secret "is no different" from an environment variable; secrets are hidden only from
  Wrangler and the dashboard after they are set
  ([Secrets](https://developers.cloudflare.com/workers/configuration/secrets/), read 2026-10-24).
- Searches of the Cloudflare documentation on 2026-10-24 for "confidential computing", "attestation",
  "enclave" and "trusted execution environment" returned no Workers feature. This is a negative result from a
  search, not proof of absence; re-check before any public claim changes. 1Password's equivalent
  (decryption inside an attested AWS Nitro enclave) has no counterpart here
  ([1Password study](#25-sources-read-for-this-page), §4.2).

So the statement, used word for word in the guide and on the landing page, is:

> Pylota Identity Cloud stores vault values encrypted with keys it holds. Values are decrypted only inside
> the Pylota Identity Worker, only to fill, compute a code, sign or seal a packet for a consumer you
> registered, and never returned to an agent, a model or an API key. Cloudflare, which runs the Worker, and
> TREFT LTD, which operates Pylota Identity Cloud, are inside this trust boundary. The vault is not
> zero-knowledge and not end-to-end encrypted. A partner can hold an extra unlock key so that a copy of the
> database and the deployment's master key is not enough to decrypt its tenants' values.

Who can decrypt, by custody mode ([§4.6](#46-custody-modes)):

| Holder | `operator` custody (default) | `partner` custody (opt-in) |
|---|---|---|
| A thief with a D1 copy only | No: every value is sealed under keys that live outside D1 | No |
| A thief with a D1 copy and `PM_MASTER_KEY` | Yes | No: the tenant's private key is also wrapped by the partner's unlock key |
| The operator (TREFT LTD), or anyone compelled to act as it | Yes, by deploying code that decrypts | Only during requests in which the partner presents its unlock key |
| Cloudflare | As for the operator: the Worker runs on its machines | As for the operator |
| An agent, a model, an MCP client, an identity or tenant key | No route returns a value | No |
| A registered consumer | Only the fields of one phase of one ticket it holds, sealed to its own key | The same |

The words "zero-knowledge", "end-to-end" and "we cannot see your passwords" must never appear in Pylota
Identity's documentation, site or SDK descriptions (`xtask::docs_forbidden_claims` scans `docs/` and
`site/` for them).

### 1.3 What sits where

```text
 agent (model) ──MCP/REST──▶ vault_request_grant · vault_list_items           (handles, never values)
                                    │
 person ──console / partner app──▶ approve grant, approve use                (decisions reserved for people)
                                    │
 integrator orchestrator ──REST (vault:use)──▶ POST …/vault/uses ──▶ ticket vtk_… (to the orchestrator once)
   (deterministic trigger: a login form, an OTP field, a session start)        │
                                    └── passes the ticket to its consumer ──┐  (never through the model)
                                                                            ▼
 consumer (browser fleet / egress proxy / executor) ──signed request + ticket──▶ POST /v1/vault/consumer/redeem
                                    ◀── HPKE packet sealed to the consumer's X25519 key ───┘
   fills at the exact origin, reports the outcome, writes back a refreshed session
```

Every step writes a row to the tenant's hash-chained vault audit and an event to the tenant's log
([§14](#14-audit-and-events)).

### 1.4 Credentialed fetch is deferred

A credentialed fetch would have the Worker make an outbound request to a host on the item, inject the secret
and return the response. It is P2, for five reasons:

1. **The response is the leak.** A host on the item that echoes a request (a debug endpoint, an error page
   that repeats the `Authorization` header) returns the secret to the caller. Redacting exact, base64 and
   URL-encoded forms is best effort; a host can return a hash, a substring or a reversible transform.
2. **The vault would become a general HTTP proxy** on Cloudflare's addresses for tenant-chosen destinations,
   with the SSRF guard of [Security § 9](security.md#9-ssrf-controls), redirect handling, response size and
   time limits, and the abuse and attribution questions of traffic that third parties see as coming from
   Pylota Identity.
3. **Responses carry personal data back** toward models, which this deployment would then have to screen
   like mail.
4. **Pylota does not need it in v1.** Its API calls already run in its own servers; an `executor` consumer
   ([§8.1](#81-consumer-kinds)) receives one secret per ticket, sealed to its key, and injects it into the
   request it makes itself, so the response never passes through Pylota Identity.
5. **The owner builds alone** ([ADR 0015](../adr/0015-solo-operator.md)); a second outbound surface doubles the
   review load of T2 code.

It is revisited, with an ADR, when a customer without its own executor asks for it.

## 2. Concepts and lifecycle

```text
Tenant (ten_)
  ├─ Tenant vault key (TVK)       X25519 key pair; private half sealed under PM_MASTER_KEY
  │                               (and under the partner's unlock key in partner custody)
  ├─ Consumer (vcn_)              tenant-level consumers; partner- and platform-level ones reach many tenants
  ├─ Item (vit_)                  login | api_key | session | managed_secret; tenant- or identity-scoped
  │    ├─ Version (1, 2, …)        current | previous | pending; its own data key (DEK), sealed to the TVK
  │    └─ Grant (vgr_)             identity × operations × origins × expiry; pending → active → revoked/expired
  ├─ Use (vus_)                   one request to use an item: pending_approval → approved → in_use → closed
  │    └─ Ticket (vtk_…)           the capability a consumer redeems, phase by phase
  └─ Vault audit (vad_)           append-only, hash-chained, one row per change, refusal and report
```

| Object | Created by | Decided by | Ends |
|---|---|---|---|
| Item | A person, the integrator, an agent for its own identity (values write-only), or the ledger approval that creates a login for a new account | — | Deleted by `vault:admin` (crypto-shredded), or by erasure |
| Grant | An agent's request (`vault:request`), or a person or partner key creating it already approved | A person, or a platform or partner key under `vault.key_approval` with `on_behalf_of` | Expiry (default 7 days, at most 90), revocation, the ledger entry's closure, the item's deletion |
| Use | The orchestrator (`vault:use`), from a deterministic trigger | Approval as the item and tenant policy require ([§6](#6-uses-and-approval)) | Closed when its phases are done, its window elapses or anything it depends on is revoked |
| Ticket | Minted once per approved use, to the orchestrator | — | One redemption per phase, first within 60 seconds of minting |
| Consumer | `vault:admin` | Activated by a person or a platform key | Suspension, revocation |

## 3. Items

### 3.1 Fields

```json
{
  "id": "vit_01JAD4K8V4QW7X2M5N6P8R0T1Y",
  "tenant_id": "ten_01J9…",
  "identity_id": "idn_01J9…",
  "kind": "login",
  "title": "TfL road user charging",
  "account_id": "sac_01JA…",
  "username": "compliance.brightwell@pylotamail.com",
  "origins": ["https://tfl.gov.uk"],
  "approval": "per_run",
  "state": "active",
  "health": "ok",
  "health_reason": null,
  "current_version": 3,
  "has": { "password": true, "totp": true, "recovery_codes": false, "email_otp": true, "sms_otp": false },
  "otp_channel": { "kind": "email", "address": "compliance.brightwell@pylotamail.com", "sender_domains": ["tfl.gov.uk"] },
  "rotation": { "remind_after_days": 90, "last_rotated_at": "2026-10-20T09:00:00Z" },
  "session": null,
  "created_by": { "key_id": "key_01J9…", "user_id": null },
  "first_used_at": "2026-10-21T08:00:00Z",
  "last_used_at": "2026-10-24T08:00:00Z",
  "created_at": "2026-10-20T09:00:00Z",
  "updated_at": "2026-10-24T08:00:00Z"
}
```

| Field | Rule |
|---|---|
| `id` | `vit_` + ULID. A handle, not a capability: it is useless without a grant, an approved use, a ticket and a consumer key |
| `identity_id` | Set: an **identity-scoped** item, usable only by that identity (an agent's own account). `null`: a **tenant-scoped** item, usable by the identities it is granted to (an operator's account shared with agents). From the path and the key's scope, never the body |
| `kind` | `login`, `api_key` or `session`; `managed_secret` exists only for the deployment's own use ([§13.2](#132-managed-secrets)) |
| `title` | 1–120 characters, chosen by the creator; untrusted text, escaped in the console; never in audit rows or logs |
| `account_id` | The service-ledger entry (`sac_`) the item belongs to. Required for every identity-scoped item; for a tenant-scoped item, `null`, and each grantee needs its own approved entry ([§3.3](#33-ledger-binding)) |
| `username` | 0–254 printable characters. Not a secret: it is shown in the console and returned by the API (the ledger already shows `account_identifier` in clear). It is personal data: never in audit rows or logs |
| `origins` | 1–10 exact origins ([§3.2](#32-exact-origins)) |
| `approval` | `grant`, `per_run` or `per_use` ([§6.1](#61-the-approval-decision)). Default `per_use` |
| `state` | `active` or `archived`. An archived item refuses every use, keeps its values and can be made active again by `vault:write` (when its ledger entry is approved) |
| `health`, `health_reason` | `ok`; `needs_auth` (`session_rejected`, `session_expired`); `needs_attention` (`credentials_rejected`, `otp_rejected`); `needs_rotation` (`value_exposed`, `rotation_due`, `manual`). Set by consumer reports, expiry and the daily job ([§10.4](#104-health-and-hand-off)) |
| `current_version` | The version uses decrypt. Older versions: [§11](#11-rotation) |
| `has` | Which sealed fields exist, and whether a one-time-code channel is configured. Never a value |
| `otp_channel` | Login items only, optional: `{ "kind": "email", "address", "sender_domains" }` or `{ "kind": "phone", "number_id", "senders" }` ([§9](#9-one-time-codes)) |
| `rotation` | `remind_after_days` (null, or 7–730) and `last_rotated_at` |
| `session` | Session items only: `{ "ttl_seconds", "expires_at", "size_bytes", "cookie_count", "origin_count" }` |
| `first_used_at` | Set by the first successful redemption; while `null`, a `signup` form may be filled ([§8.6](#86-redemption-checks)) |

Values are write-only. They are given on create and on rotation, and never returned:

| Kind | Sealed fields (the `purpose` in the AAD) | Values accepted |
|---|---|---|
| `login` | `password`, `totp_seed`, `recovery_codes` | `password` as a string of 1–1,024 bytes, or `{ "generate": { "length": 16–128, "classes": ["lower","upper","digit","symbol"] } }` (default 24 characters, all four classes, from `platform::Rng`); `totp` as an `otpauth://totp/` URI (secret 16–128 base32 characters; `algorithm` SHA1, SHA256 or SHA512; `digits` 6 or 8; `period` 15–120, default 30); `recovery_codes` as 1–20 strings of at most 64 characters |
| `api_key` | `secret` | `secret` as a string of 1–16,384 bytes; `inject` as `{ "scheme": "bearer" \| "basic" \| "header", "header_name"?, "username"? }`; P1: `sign` as `{ "scheme": "hmac_sha256", "encoding": "hex" \| "base64" }` or `{ "scheme": "aws_sigv4", "access_key_id", "region", "service" }` ([§12](#12-signing)) |
| `session` | `session_state` | `state` as `{ "cookies": [...], "origins": [{ "origin", "local_storage": [{ "name", "value" }] }] }` in the shape of Playwright's `storageState`; filtered to the item's origins before sealing ([§10.1](#101-what-a-session-item-holds)) |

### 3.2 Exact origins

`core::vault::origin::canonicalise` turns each listed URL into `https://{host}[:{port}]`:

1. Scheme `https` only. No userinfo, path other than `/`, query or fragment (`400 invalid_request`,
   `details.errors[].path = "origins[i]"`).
2. The host is a DNS name of at least two labels, converted to an IDNA A-label and lower-cased. IP literals in
   any notation are refused (the rules of [Security § 9.1](security.md#91-url-rules-at-create-update-and-every-attempt)
   rule 2), and so are the names of rule 3 (`localhost`, `.internal`, `.test` and the others), the deployment's
   own hosts and the platform domain.
3. A host label in an A-label form (`xn--`) is accepted only when its Unicode form passes the confusable check
   of [Identities and domains › Username validation](identity-domains.md#username-validation) against the
   item's ledger `service_domain`; a mixed-script host that skeleton-matches the service domain is refused
   with `reason = "confusable_origin"`.
4. Port 443 is dropped; any other port is kept and must be 1024–65535.
5. No wildcards. `www.example.com` and `example.com` are different origins; neither implies the other, and no
   subdomain is implied.

Matching at redemption is byte equality of canonical origins: the origin the consumer reports, and the
origins of the frame and the top-level page, must each equal one of the item's (or the grant's narrower)
origins ([§8.6](#86-redemption-checks)).

### 3.3 Ledger binding

Every item an agent can use is tied to an approved entry of the [service sign-up ledger](service-accounts.md),
so an agent signs in only where a person approved that account:

- **Identity-scoped items** carry `account_id`. The entry must belong to the same identity and be `approved`
  when the item is created, when a grant is approved, when a use is created and when a ticket is redeemed.
- **Tenant-scoped items** carry no `account_id`. A grant to an identity needs an `approved` entry **of that
  identity** for the service; the use and the redemption re-check it.
- **Origin rule.** Each origin's registrable domain (the same `psl` function as `service_domain`) must equal
  the entry's `service_domain` or one of its `login_domains`, a new ledger field of at most 5 organisational
  domains where the account signs in (for a service whose sign-in page is on another domain). The console's
  approve page lists `login_domains` apart, as it lists extra `sender_domains`, because approving them lets a
  fill happen there ([Service sign-up ledger](service-accounts.md); staged change).
- **Closing the entry** archives every item linked to it (`state = "archived"`, `health_reason` unchanged),
  revokes every grant that relied on it and closes every open ticket (`close_reason = "ledger_closed"`), in the
  D1 batch of the closure. Deleting the entry does the same and sets `account_id` to `NULL` on its items.
- **Refused domains.** An origin whose registrable domain is on the refused list is refused at item create and
  at every later check (`422 vault_kind_refused`, `details.reason = "refused_domain"`). The built-in list holds
  `account.gov.uk` (GOV.UK One Login, whose terms tell users not to use the service "with someone else's
  registered details" unless they have the authority to do so;
  [terms](https://signin.account.gov.uk/terms-and-conditions), read 2026-10-24 for the browser study).
  `PM_VAULT_REFUSED_DOMAINS` adds to it. Other government identity services are candidates that counsel and the
  owner decide ([§23](#23-questions-for-counsel)).

This is the vault's own rule, whatever the tenant's `accounts.require_approval` says: the vault never uses a
credential at a service no person approved. An integrator keeps its own site policy as well (Pylota's
`browser_automation_targets`, where `prohibited` wins); the consumer contract requires that check
([§8.5](#85-the-consumer-contract)).

### 3.4 Creating an item

| Route | Who | Scope |
|---|---|---|
| `POST /v1/identities/{identity_id}/vault/items` | `vault:write`, any key level (an identity key only for its own identity) | An identity-scoped item |
| `POST /v1/tenants/{tenant_id}/vault/items` | `vault:write`, platform, partner or tenant key | A tenant-scoped item |
| The ledger approval with `vault_login` ([§3.5](#35-a-login-born-from-a-ledger-approval)) | The approver of the entry | An identity-scoped login and its first grant |

The handler, in order: `422 vault_disabled` unless `PM_VAULT = "on"` and the tenant's `vault.allowed` is
`true`; `403 tenant_suspended`; for an identity item `409 identity_paused`; validate the body and the refusals
of [§3.6](#36-refused-items-and-values); canonicalise origins and check the ledger binding (a missing or
unapproved entry is `422 invalid_request` with `details.reason = "ledger_entry_not_approved"`, and an entry of another identity is `404 account_not_found`); check the limits (`422 vault_item_limit_reached`, at most 100
items per identity and the tenant's `vault.max_items`, default 1,000, in an `INSERT … SELECT … WHERE` guard);
create the tenant vault key on first use ([§4.2](#42-the-tenant-vault-key)); then seal ([§4.3](#43-data-keys-and-field-ciphertext)):

1. generate the version's DEK from `platform::Rng`;
2. seal each given field with the DEK under its purpose;
3. seal the DEK to the tenant's public key;
4. for a session, write the sealed state to R2 at `t/{tenant_id}/vault/{item_id}/1` first;
5. one D1 batch: the item row, the version row, the audit row `item.created` and the event
   `vault.item.created`, guarded by the audit sequence ([§14.2](#142-appending)).

`Idempotency-Key` is optional, as for ledger requests; a replay returns the item. The response is `201` with
the item, which never carries a value. Neither does the idempotency record, which stores the response.

Creating an item needs no unlock key in either custody mode, because sealing uses only the tenant's public
key ([§4.6](#46-custody-modes)).

### 3.5 A login born from a ledger approval

`POST /v1/identities/{identity_id}/accounts` gains an optional `vault_login`:

```json
{ "service_domain": "example-insurer.co.uk", "account_identifier": "compliance.brightwell@pylotamail.com",
  "purpose": "Download policy documents for fleet renewals.",
  "vault_login": { "origins": ["https://portal.example-insurer.co.uk"], "password": { "generate": {} },
                   "email_otp": true, "approval": "per_run" } }
```

The entry is stored with the request as usual. When a person approves the entry (or a key under the rule of
[Service sign-up ledger §4](service-accounts.md#4-approval)), the same D1 batch creates the identity-scoped
login item (generated password; `otp_channel` set to the entry's address and `sender_domains` when
`email_otp` is true) and an active grant to the requesting identity for `fill` on those origins, expiring after
90 days. The approve page shows both: "Approving also creates a login this agent can use to sign in at …".
So an agent that signs up for a service never holds the password it signs up with: the vault generates it,
and the browser consumer fills the sign-up form ([§8.6](#86-redemption-checks), `form_kind = "signup"`). A
rejected or expired entry creates nothing. The MCP tool `mail_request_account` accepts the same argument.

### 3.6 Refused items and values

`core::vault::refuse` runs on every create and rotation:

| Refused | How it is recognised | Answer |
|---|---|---|
| A kind other than the three | The enum | `400 invalid_request` |
| Payment cards and bank details | A kind named `card`, `credit_card`, `bank_account` or `passkey` in the request (`422 vault_kind_refused`, `details.reason = "payment_card"`, `"bank_account"` or `"passkey"`), and any sealed field that, with spaces and dashes removed, is 13–19 digits passing the Luhn check (`details.reason = "payment_card_detected"`), so a card cannot be stored as a "password" | `422 vault_kind_refused`, with the `fix` "Keep payment cards with your payment provider." |
| Passkeys | No field accepts a WebAuthn private key; the `passkey` kind is refused. The CDP WebAuthn domain is a test tool: `addCredential` takes a credential's raw private key and `automaticPresenceSimulation` makes presence tests succeed, defeating both properties that make a passkey resist phishing (`devtools-protocol@0.0.1367902`, WebAuthn domain, read 2026-10-24 for the browser study); no specification for agents exists yet (the FIDO Alliance formed its agentic authentication working group on 2026-04-28 and had published none when read on 2026-10-24) | `422 vault_kind_refused` (`passkey`) |
| Bot-challenge clearance | Cookies named in the clearance list are dropped from every session state, on create, import and write-back, and counted: the built-in list is `cf_clearance`, which "stores the proof of challenge passed" ([Cloudflare cookies](https://developers.cloudflare.com/fundamentals/reference/policies-compliances/cloudflare-cookies/), read 2026-10-24); `PM_VAULT_CLEARANCE_COOKIES` adds names | Dropped, never refused; `clearance_cookies_dropped` in the response and the audit row |
| Refused domains | [§3.3](#33-ledger-binding) | `422 vault_kind_refused` (`refused_domain`) |

## 4. Keys and envelopes

### 4.1 The hierarchy

```text
PM_MASTER_KEY (two slots, Security § 6.2)
  └─ pm1 ── Tenant vault key, private half (X25519, 32 bytes)        D1 vault_keys.private_sealed
             [partner custody: pm1( AES-256-GCM(unlock key, private) ) in vault_custody_slots]
       public half (in clear) ── HPKE seal ──▶ version DEK (32 bytes)  D1 vault_item_versions.dek_sealed
                                                 └─ AES-256-GCM ──▶ fields   D1 fields_json / R2 session object
  Ticket secret (32 bytes, never stored) ── AES-256-GCM ──▶ DEK copy   D1 vault_uses.dek_ticket_sealed (≤ the ticket's life)
```

Every key is generated from `platform::Rng`; none is derived from another (SEC-3). The one documented
exception is the HPKE key schedule itself (RFC 9180 §5), which derives a one-time key from an ephemeral
X25519 exchange for each seal; it derives no stored secret ([ADR 0021](../adr/0021-agent-vault.md), decision 4).

### 4.2 The tenant vault key

- **Created** on the tenant's first vault write: 32 bytes from `platform::Rng` become an X25519 static secret
  (`x25519-dalek` 3.0.0, the version `hpke` 0.14.1 depends on; crates.io, read 2026-10-24); its public key is
  computed; `kid` is the RFC 7638 thumbprint of the public JWK (`{"crv":"X25519","kty":"OKP","x":…}`). Two
  first writes racing insert under the unique index `vault_keys_one_active`; the loser zeroises its secret and
  reads the winner, as identity keys do ([Agent signing keys §2](agent-keys.md#2-keys)).
- **Stored** in `vault_keys`: `public_key`, `kid`, `custody`, `state` (`active` or `retiring`), and for operator
  custody `private_sealed` = the pm1 envelope of the 32 bytes with AAD
  `pm1|vault_keys|private_sealed|{tenant_id}:{kid}`. `vault_keys.private_sealed` joins the sealed-column
  registry `crates/core/src/sealed.rs`, so `idvy secrets rotate-master` re-seals it like every other sealed
  value ([Security §6.2](security.md#62-rotation-procedures)); so does `vault_custody_slots.private_wrapped`,
  whose inner layer the sweep carries unopened.
- **Opened** only when a DEK must be unwrapped: at ticket minting, at signing, at reveal and at tenant-key
  rotation. It is held in memory for that one request and zeroised (`zeroize`); it is never cached in the
  isolate.
- **Rotated** by `POST /v1/tenants/{tenant_id}/vault/rotate-key` (`vault:admin`; audit `vault.key_rotated`):
  a new active key is created and the old one becomes `retiring`. New versions seal to the new key at once. A
  re-wrap sweep (the every-minute cron, at most 200 versions a run) opens each DEK sealed to a retiring key and
  seals it to the active one; under partner custody it cannot open the retiring key, so a version is re-wrapped
  when it is next unwrapped with the unlock key present, and `GET /v1/tenants/{tenant_id}/vault` reports
  `retiring_versions`. A retiring key with no versions left is deleted.

### 4.3 Data keys and field ciphertext

- **One DEK per item version**, 32 random bytes. It never outlives the version: rotation and write-back make
  a new version with a new DEK.
- **DEK seal**: HPKE base mode (RFC 9180), suite DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, AES-256-GCM
  (suite IDs 0x0020, 0x0001, 0x0002), to the tenant's active public key, with
  `info = "pylota-identity/vault-dek/v1"` and `aad = "pv1|dek|{tenant_id}|{item_id}|{version}|{kid}"`.
  Stored as `dek_sealed = enc ‖ ciphertext` with `tvk_kid`.
- **Field ciphertext**: `pv1.{nonce}.{ciphertext}`, AES-256-GCM under the DEK with a random 96-bit nonce and
  `aad = "pv1|field|{tenant_id}|{item_id}|{version}|{purpose}"`. A login's password, TOTP seed and recovery
  codes are three ciphertexts, so a fill that needs only the password and a code never decrypts the recovery
  codes, and a ciphertext moved to another field, version, item or tenant does not decrypt.
- **Session state** is one `session_state` ciphertext stored in R2 at `t/{tenant_id}/vault/{item_id}/{version}`
  (at most 1 MiB of plaintext, [§10.1](#101-what-a-session-item-holds)), with its SHA-256 in the version row.
  Vault objects are never copied to the backup bucket (`PM_BACKUP_BUCKET`), as uploads are not
  ([Privacy §5.4](privacy.md#54-optional-r2-backup-copy)); and because their DEKs live in D1, deleting a
  version's row makes any copy of the object that survives elsewhere undecryptable.
- **Crates**: `hpke` 0.14.1 with `default-features = false` and the features `x25519`, `aes` and `alloc`
  (MIT or Apache-2.0; its dependencies include `aes-gcm` ^0.11, `hkdf` ^0.13, `sha2` ^0.11 and `x25519-dalek`
  ^3.0.0; crates.io, read 2026-10-24), with randomness supplied from `platform::Rng` through `rand_core`, never
  `getrandom`. The workspace's `aes-gcm` pin must resolve to the same 0.11 line, because `cargo deny` refuses
  duplicate versions of `aes-gcm` ([Security §11](security.md#11-supply-chain)). Spike **S-VLT1** proves the
  suite on `wasm32-unknown-unknown` against the RFC 9180 test vectors ([§24](#24-tests)); its fallback is in
  [ADR 0021](../adr/0021-agent-vault.md).

### 4.4 The ticket copy

When a ticket is minted ([§7.1](#71-minting)), the Worker opens the tenant key, unwraps the version's DEK, and
seals that DEK under the ticket's 32-byte secret with AES-256-GCM and
`aad = "pv1|ticket|{tenant_id}|{use_id}|{item_id}|{version}"`, into `vault_uses.dek_ticket_sealed`. The ticket
secret goes to the orchestrator once and is never stored; the row keeps only `SHA-256(ticket)` to find it. At
redemption the Worker opens the DEK with the ticket the consumer presents, so:

- redemption needs neither the tenant key nor the unlock key;
- a copy of D1 taken during a ticket's life still lacks the ticket secret;
- `dek_ticket_sealed` and `ticket_hash` are cleared in the batch that closes the use.

### 4.5 What the Worker holds, and for how long

| Material | In memory | Persisted |
|---|---|---|
| Tenant private key | One request (mint, sign, reveal, key rotation) | Sealed only |
| A DEK | One request | Sealed to the tenant key; and under a ticket while the use is open |
| A field value | One request (sealing it, or sealing a packet) | Sealed only |
| A ticket secret | The mint response | Never; its SHA-256 only |
| An unlock key | One request | Never |
| A packet's plaintext | While sealing it | Never |

Each buffer is a `Zeroizing<Vec<u8>>`; `core::vault` types holding values implement neither `Debug` nor
`Serialize` (`xtask::vault_types_have_no_debug`), so a value cannot reach a log line or a response body by a
derive.

### 4.6 Custody modes

| | `operator` (default) | `partner` (opt-in, [ADR 0021](../adr/0021-agent-vault.md) decision 5) |
|---|---|---|
| Who chooses | — | The tenant's own partner key, with `POST /v1/tenants/{tenant_id}/vault/custody` |
| What is stored | `vault_keys.private_sealed` (pm1) | One or two `vault_custody_slots` rows: `pm1( AES-256-GCM(unlock key, private, aad "pvu1|{tenant_id}|{kid}|{slot}") )`; `private_sealed` is `NULL` |
| Sealing (create, rotate, write-back, import) | Public key only | Public key only: no unlock key needed |
| Unwrapping (mint, sign, reveal, key rotation) | Master key | Master key and the unlock key in `X-Pylota-Identity-Vault-Unlock` |
| Console reveal | Owners and admins | Not available (the console never holds an unlock key) |
| Lost unlock keys | — | Every item is unrecoverable; nobody can recover them, by design. A second slot exists for a backup key |

**Enrolling.** `POST /v1/tenants/{tenant_id}/vault/custody` with `{ "slots": 1 | 2 }`, by the tenant's own
partner key holding `vault:admin` (any other key: `403 scope_denied`). The Worker generates one or two unlock
keys (`pvu_` + 52 Crockford base32 characters encoding 32 random bytes), wraps the tenant's private key under
each, deletes `private_sealed` in the same batch, writes the audit row `vault.custody_changed` and returns the
keys once (`200`, never replayed: the idempotency record stores the response without them, as for API key
secrets, J19). On a tenant with no vault yet, the tenant key is created in partner custody. On a tenant with a
vault, the old operator-sealed row stays recoverable from D1 Time Travel for 30 days
([D1 limits](https://developers.cloudflare.com/d1/platform/limits/), read 2026-10-24); the response says so in
`operator_copy_restorable_until`.

**Presenting.** The header is accepted only from the tenant's own partner keys, only on the unwrapping routes
(`POST …/vault/uses` when it mints, `POST /v1/vault/uses/{use_id}/ticket`, `…/signature`,
`POST /v1/tenants/{tenant_id}/vault/rotate-key` and `DELETE …/vault/custody`), and only over the API host. On
any other route, or from any other key, it is `400 invalid_request` (`details.reason = "unlock_not_accepted"`),
so clients do not spread it. Missing where needed: `403 vault_unlock_required`. A key that opens no slot (the
AES-GCM tag fails): `403 vault_unlock_invalid`, counted in `vault_unlock_failures_total` and limited by `RL_API`.
The header is never logged, stored, put in an idempotency record or echoed.

**Releasing.** `DELETE /v1/tenants/{tenant_id}/vault/custody` with a valid unlock key re-seals the private key
under the master key (`private_sealed`), deletes the slots and returns to operator custody (audit
`vault.custody_changed`).

**Erasure.** Tenant erasure deletes the slots first ([§16](#16-retention-and-erasure)). Once the partner
discards its unlock keys, a D1 restore brings back only slots that nobody can open.

## 5. Grants

A grant says that one identity may use one item for some operations at some of its origins until a time. An
agent asks for it; a person decides.

```json
{ "id": "vgr_01JAD5…", "tenant_id": "ten_01J9…", "item_id": "vit_01JAD4…", "identity_id": "idn_01J9…",
  "operations": ["fill"], "origins": ["https://tfl.gov.uk"], "status": "active",
  "purpose": "Check PCN status each morning.", "max_uses": null, "use_count": 12,
  "requested_by_key_id": "key_01J9…", "decided_by_key_id": "key_01J8…", "decided_by_user_id": null,
  "on_behalf_of": "usr_8812", "note": null, "decided_at": "…", "expires_at": "2026-10-31T09:00:00Z",
  "revoked_at": null, "revoke_reason": null, "created_at": "…", "updated_at": "…" }
```

| Field | Rule |
|---|---|
| `operations` | A non-empty subset of the item kind's operations: `login` → `fill` and `rotate_fill` (P1); `session` → `session_load` and `session_capture`; `api_key` → `inject` and `sign` (P1). `managed_secret` items take no grants (`422 invalid_request`, `reason = "managed_item"`) |
| `origins` | A non-empty subset of the item's origins; default all of them. An item origin removed later leaves the grant with the rest; a grant left with none is revoked (`origins_removed`) |
| `status` | `pending_approval`, `active`, `denied`, `revoked` or `expired` |
| `purpose` | 1–500 characters written by the requester: untrusted, escaped, never in audit rows or logs |
| `max_uses` | Optional, 1–100,000; a use beyond it is `403 vault_not_granted` (`max_uses_reached`) |
| `expires_at` | While pending: `created_at` + 7 days, then `expired`. Once active: 1 hour to 90 days after approval, default 7 days, set by the approver (`expires_in`) |
| `on_behalf_of` | The person in the caller's own application who decided (as for drafts: 1–128 printable ASCII characters with no `@`). Required when an API key approves under `vault.key_approval` |

**Requesting** (`POST /v1/identities/{identity_id}/vault/grants`, `vault:request`, any key level; an identity
key only for its own identity): `{ "item_id", "operations", "origins"?, "purpose", "max_uses"? }`. Refused
before the batch: `422 vault_disabled`, `403 tenant_suspended`, `409 identity_paused`; an item the key cannot
reach, or an identity-scoped item of another identity, `404 vault_item_not_found`; an operation the kind does
not take or an origin not on the item, `400 invalid_request`; no approved ledger entry of this identity for the
item's service ([§3.3](#33-ledger-binding)), `403 vault_not_granted` (`ledger_not_approved`). Then one batch
with guards: at most 20 pending requests per identity and 50 non-terminal grants per item
(`422 vault_grant_limit_reached`); no pending or active grant of the same identity and item that already
covers the operations and origins (`409 vault_grant_exists`, `details.grant_id`, `details.status`). It writes
the audit row `grant.requested` and the event `vault.grant.requested`, and answers `201`. The MCP tool
`vault_request_grant` maps to it; there is no tool to approve.

**Creating it approved.** The same route with `"approve": true`, an optional `expires_in` and, from an API key,
`on_behalf_of`, by a caller that holds `vault:approve` and passes the decision rule below, writes the grant
`active` in one batch with the audit rows `grant.requested` and `grant.approved` (`details.direct = true`). This
is how a person in the console, or Pylota's server acting for an operator who decided in Pylota's app, gives an
agent access without a request first.

**Approving** (`POST /v1/vault/grants/{grant_id}/approve`, `vault:approve`; platform, partner or tenant key;
never an identity key) with `{ "expires_in"?, "note"?, "on_behalf_of"? }`:

1. **Who.** Approving lets an agent use a credential, so it is a decision reserved for people
   ([Workspace policy §3](workspace-policy.md#3-decisions-reserved-for-people)):
   `auth::people::key_may_decide(Decision::ApproveVault, …)` is true when `PM_QUARANTINE_KEY_RELEASE` is `on` or
   `PM_CONSOLE` is `off`, or, where the tenant's `vault.key_approval` is `true`, for a platform key or the
   partner key of the tenant's own partner (never a tenant key), and then `on_behalf_of` is required
   (`400 invalid_request`, path `on_behalf_of`). Otherwise `403 permission_denied` with
   `details.reason = "person_required"`, and a signed-in owner or admin approves in the console after
   re-authentication. `quarantine.key_release` and `drafts.key_approval` do not open this decision.
2. **Not your own.** The key that requested the grant never approves it: `403 permission_denied`,
   `details.reason = "self_approval"`.
3. **State.** `UPDATE … WHERE id = ?1 AND status = 'pending_approval' AND expires_at > ?now`, guarded with the
   audit row and the event in one batch. The same decision again answers `200` and writes nothing. A grant in
   any other state, or pending past its 7 days (set `expired` in the same request), answers
   `409 vault_not_pending` with `details.status`, the one conflict code for grants and uses alike.
4. **Ledger.** The entry is re-checked in the batch: closed meanwhile → `403 vault_not_granted`
   (`ledger_not_approved`).

Denying (`…/deny`, `{ "note"? }`) is always allowed for a key with `vault:approve`, because it can only keep a
credential away from an agent.

**Revoking** (`POST /v1/vault/grants/{grant_id}/revoke`, `{ "reason"? }`): `vault:approve`, or `vault:request`
from a key of the grantee identity (an agent may give up its own access). An active grant becomes `revoked`; the
same batch closes every open use that relied on it (`close_reason = "grant_revoked"`) and writes
`grant.revoked`. Revoking a revoked, denied or expired grant answers `200` with no change.

**Expiry.** The global retention job sets active grants past `expires_at` to `expired` (audit
`grant.expired`, event `vault.grant.expired`); every use and redemption also checks `expires_at` itself, so a
grant stops working at the second it expires whether or not the job has run.

**Reading.** `GET /v1/identities/{identity_id}/vault/grants` (`vault:read`; filters `status`, `item_id`),
`GET /v1/tenants/{tenant_id}/vault/grants` (`vault:read`; platform, partner or tenant key; the approval queue,
oldest pending first) and `GET /v1/vault/grants/{grant_id}`. An identity key sees only its own identity's
grants; anything outside the key's scope is `404 vault_grant_not_found`.

## 6. Uses and approval

A use is one request to use an item now. The integrator's orchestrator creates it when its own deterministic
code sees the need (a password field on an authorised origin, an OTP field, the start of a session-backed run,
an outbound API request); a model never does ([§17.3](#173-who-can-start-a-fill)).

### 6.1 The approval decision

Three levels, ordered `grant` < `per_run` < `per_use`:

| Level | A use needs a person when |
|---|---|
| `grant` | Never: the active grant is the approval. For unattended, low-risk reads, such as a daily status check |
| `per_run` | No approved use of the same identity, item and consumer with the same `run_id` was approved in the last 60 minutes |
| `per_use` | Always |

The **effective level** is the strictest of: the item's `approval`; the tenant's `vault.min_approval` (a guard
field, default `grant`); and the operation's floor (`rotate_fill` and `session_capture` are always
`per_use`). `core::vault::policy::approval_needed(level, prior_run_approval) -> Option<ApprovalReason>` is the
pure function; its answer is stored as `approval_reason` (`per_use`, `per_run`, `operation`, `tenant_floor`).

**Changing an item's level.** A `PATCH` that sets a stricter level is always accepted. A looser one (for
example `per_use` to `grant`) is accepted from a person, from a platform key, or from the tenant's partner key
when `key_may_decide(Decision::ApproveVault, …)` is true for it; from any other key it is `403
permission_denied` with `details.reason = "person_required"` and `details.field = "approval"`. The same rule
covers `vault.min_approval`, which joins the guard fields of
[Workspace policy §2](workspace-policy.md#2-classes-and-ceilings).

### 6.2 Creating a use

`POST /v1/identities/{identity_id}/vault/uses` (`vault:use`, any key level; `Idempotency-Key` required):

```json
{ "item_id": "vit_01JAD4…", "operation": "fill", "phases": ["credentials", "email_otp"],
  "consumer_id": "vcn_01JAC2…", "run_id": "run_8f2c41", "origin": "https://tfl.gov.uk",
  "trigger": "login_form", "purpose": "Check PCN status for WM12ABC", "capture_into": "vit_01JAD9…",
  "run_window_seconds": null }
```

| Field | Rule |
|---|---|
| `operation`, `phases` | [§7.2](#72-operations-and-phases); `phases` is a non-empty subset of the operation's, without repeats |
| `consumer_id` | An `active` consumer that reaches the tenant and whose kind allows the operation ([§8.1](#81-consumer-kinds)) |
| `run_id` | 1–128 printable ASCII characters without `@`: the integrator's identifier of the run (for Pylota, the browser run). Opaque to the vault |
| `origin` | One canonical origin of the item and of the grant |
| `trigger` | `login_form`, `signup_form`, `reauth_prompt` or `otp_field` (`fill`); `session_start` (`session_load`); `person_handoff` (`session_capture`, or a `fill` into a session a person is watching); `api_request` (`inject`); `password_change` (`rotate_fill`); `sign_request` (`sign`). An enumeration, never free text |
| `purpose` | 1–200 characters shown to the approver as untrusted text; never in audit rows or logs |
| `capture_into` | Optional, for `fill`: a session item of the same identity scope, ledger entry and origins, into which the consumer may write the session it gets by signing in ([§10.3](#103-write-back-and-rotation-after-use)) |
| `run_window_seconds` | `session_load`, `session_capture` and `inject` only: 300–7,200, default 1,800 ([§7.3](#73-windows)) |

Checks, in order, all before anything is written: `422 vault_disabled`; `403 tenant_suspended`; `409
identity_paused`; the item (`404 vault_item_not_found` when out of reach; `403 vault_refused` with
`item_archived`, `item_needs_auth` for a session that needs sign-in, `item_needs_rotation` for an item whose
value was exposed, `session_expired`); the operation and phases (`400 invalid_request`); an active grant of this
identity covering the item, operation and origin (`403 vault_not_granted` with `no_grant`,
`operation_not_granted`, `origin_not_granted`, `grant_expired` or `max_uses_reached`); the ledger entry
(`403 vault_not_granted`, `ledger_not_approved`); the consumer (`404 vault_consumer_not_found`,
`403 consumer_not_active`, `403 vault_refused` with `consumer_kind_not_allowed`); a session item with a use
already `in_use` (`403 vault_refused`, `session_in_use`); `RL_VAULT` (120 uses a minute per identity,
`429 rate_limited`); at most 100 pending uses per tenant (`429 rate_limited`, `details.bucket =
"vault_pending"`).

Then one batch, guarded by the audit sequence: the use row, the audit row `use.requested`, and either
`use.approval_requested` with its event (when a person must decide) or `use.approved` (when the effective level
needs none, `approval_reason = null`, or a per-run approval covers it, `approved_by = "run"`). When it is
approved at once, the same request mints the ticket ([§7.1](#71-minting)) and returns it.

Response `201`:

```json
{ "use": { "id": "vus_01JAD6…", "status": "approved", "approval_reason": null, "digest": "c41a…",
           "deadline_at": null, "…": "…" },
  "ticket": "vtk_7k2m9q4xa0bc_…", "ticket_expires_at": "2026-10-24T08:00:60Z" }
```

`ticket` is `null` while the use waits for approval. An idempotent replay returns the use with
`ticket: null` and `ticket_replayed: false`, never the ticket; the orchestrator mints a fresh one with
`POST /v1/vault/uses/{use_id}/ticket` ([§7.1](#71-minting)).

### 6.3 The digest

`digest = hex(sha256(canonical_json({ v: 1, use_id, tenant_id, identity_id, item_id, operation, phases (sorted),
origin, consumer_id, run_id, capture_into })))`, with the canonical JSON of
[Outbound › Idempotency fingerprint](outbound.md#idempotency-fingerprint). A use never changes after creation,
so the digest names exactly what the approver saw; an approval that passes a digest must match it.

### 6.4 Approving, denying and cancelling

`POST /v1/vault/uses/{use_id}/approve` (`vault:approve`; never an identity key) with
`{ "digest", "note"?, "on_behalf_of"? }`, under the same three rules as a grant ([§5](#5-grants)): the
decision is reserved for people (`Decision::ApproveVault`, with `vault.key_approval` and `on_behalf_of` for
platform and partner keys); the key that created the use never approves it (`self_approval`); the use must be
`pending_approval` and before `deadline_at`, otherwise `409 vault_not_pending` (a use found past its deadline is
set `expired` in the same request). `digest` is required; one that differs from the stored digest is
`400 invalid_request` (path `digest`). The batch sets
`approved`, the decider, `approved_at` and, for `per_run`, `run_approval_until = approved_at + 60 min`, with the
audit row and `vault.use.approved`. Approving does not return a ticket: the approver is not the orchestrator. The
orchestrator learns of the approval from the event (webhook, feed or stream) or by polling
`GET /v1/vault/uses/{use_id}`, then mints.

`deadline_at = created_at + vault.approval_deadline_seconds` (60–900, default 600). The every-minute vault cron
sets pending uses past their deadline to `expired` (`vault.use.expired`).

`POST /v1/vault/uses/{use_id}/deny` (`vault:approve`, `{ "reason"? }`, any key holding it) sets `denied`.
`POST /v1/vault/uses/{use_id}/cancel` (`vault:use`) sets a pending or approved use `canceled`, and closes an
`in_use` one (`close_reason = "canceled"`).

### 6.5 What the approver sees

The console's approval page, and the data Pylota's app reads from `vault.use.approval_requested` and
`GET /v1/vault/uses/{use_id}`, show: the identity's display name and address; the item's title, kind and
username; the origin; the operation and phases; the consumer's name and hosting; the run ID; the requesting key's
name; the purpose, as untrusted text in a quoted, escaped block; and, for `per_run`, "Approving covers further
sign-ins by this agent in this run for 60 minutes". There is no "approve all" control, in the console or the
API: every use is decided on its own row.

### 6.6 Reading

`GET /v1/identities/{identity_id}/vault/uses` (`vault:read`; filters `status`, `item_id`, `run_id`),
`GET /v1/tenants/{tenant_id}/vault/uses` (`vault:read`; platform, partner or tenant key; default
`status=pending_approval`, oldest first: the approval queue) and `GET /v1/vault/uses/{use_id}`. A use never
shows its ticket, its ticket hash or its sealed DEK.

## 7. Tickets and phases

### 7.1 Minting

`POST /v1/vault/uses/{use_id}/ticket` (`vault:use`; in partner custody also `X-Pylota-Identity-Vault-Unlock`
from the tenant's partner key). Allowed while the use is `approved` and no phase has been redeemed; minting
again replaces the previous ticket, which stops matching at once. It re-checks the grant, the ledger entry, the
item, the consumer, the identity and the tenant as at creation, then:

1. draws a ticket `vtk_{lookup}_{secret}`: `lookup` is 12 characters of lower-case Crockford base32 (60 random
   bits) and `secret` 52 characters encoding 32 random bytes, the format of API keys
   ([Security §4.1](security.md#41-key-format));
2. opens the tenant key (with the unlock key in partner custody), unwraps the current version's DEK and seals it
   under the 32 secret bytes ([§4.4](#44-the-ticket-copy)); zeroises the tenant key and the DEK;
3. for an `email_otp` phase, sends `MailboxRequest::RegisterWait` for each of the item's `sender_domains`, with
   `ttl_ms` = the use's window plus 10 minutes, so the code is not unsolicited under
   [Inbound › E5](inbound.md#verification-codes-and-unsolicited-otp-e5);
4. one batch: `ticket_lookup`, `ticket_hash = hex(sha256(ticket))`, `ticket_version` (the item version sealed),
   `dek_ticket_sealed`, `ticket_issued_at`, `redeem_by = now + 60 s`, the audit row `ticket.issued` and its event;
5. answers `200 { "ticket", "ticket_expires_at", "phases", "use_id" }`. The ticket appears in no other response,
   log, event, audit row or idempotency record.

### 7.2 Operations and phases

| Operation | Kind | Phases a consumer redeems, each at most once | Write-back | Consumer kinds |
|---|---|---|---|---|
| `fill` | `login` | `credentials` (username and password), `totp`, `email_otp`, `sms_otp`: any non-empty subset, `credentials` first when present | A session into `capture_into` | `browser`, `egress_proxy` |
| `session_load` | `session` | `session` | The refreshed session (rotation after use) | `browser`, `egress_proxy` (cookies only) |
| `session_capture` | `session` | none | One session, captured after a person signed in | `browser` |
| `inject` | `api_key` | `secret` | A new secret, when the item has `rotate_after_use` | `egress_proxy`, `executor` |
| `rotate_fill` (P1) | `login` | `credentials` (the current password), then `new_password` | The confirmation that the site accepted it | `browser` |
| `sign` (P1) | `api_key` | none: no ticket and no consumer; the signature is returned to the caller ([§12](#12-signing)) | — | — |

### 7.3 Windows

- The first phase must be redeemed within 60 seconds of minting (`redeem_by`); afterwards the use expires
  (`410 vault_ticket_expired`, `details.reason = "not_redeemed"`).
- The first redemption makes the use `in_use` and sets `closes_at` = first redemption + the window: 10 minutes
  for `fill` and `rotate_fill`; `run_window_seconds` (default 30 minutes, at most 2 hours) for `session_load`,
  `session_capture` and `inject`.
- After `closes_at`, no phase and no write-back is accepted, and the vault cron closes the use
  (`window_elapsed`). A report with `final: true` closes it earlier ([§8.7](#87-reports)).

### 7.4 What closes a use early

In the batch of the cause, by one `UPDATE … WHERE` on the item, grant, consumer, identity or tenant, setting
`status = 'closed'`, `close_reason`, and clearing `dek_ticket_sealed` and `ticket_hash`:

| Cause | `close_reason` |
|---|---|
| A new current version (rotation, write-back by another use), archive or deletion of the item | `item_rotated`, `item_archived`, `item_deleted` |
| The grant revoked or expired | `grant_revoked`, `grant_expired` |
| The ledger entry closed or deleted | `ledger_closed` |
| The consumer suspended, revoked, or its partner suspended | `consumer_suspended` |
| The identity paused or deleting; the tenant suspended or erasing | `identity_paused`, `tenant_suspended` |
| `vault.allowed` set `false`, `POST …/vault/revoke-all`, the `vault_redeem` switch turned `off` | `kill_switch` |
| A refusal at redemption ([§8.6](#86-redemption-checks)) | `refused` |
| A `value_exposed` report | `value_exposed` |
| `cancel` | `canceled` |

The switches are also read live at every redemption, so a switch works even before the batch that closes uses
has run.

## 8. Consumers

### 8.1 Consumer kinds

| Kind | Is | May run | Example |
|---|---|---|---|
| `browser` | A component that drives a browser it controls and fills forms through CDP | `fill`, `session_load`, `session_capture`, `rotate_fill` | Pylota's browser fleet supervisor |
| `egress_proxy` | A forward proxy that substitutes placeholders in permitted requests, attaches cookies and injects headers | `fill` (by placeholder substitution), `session_load` (cookies only), `inject` | Pylota's `browser-egress-proxy` |
| `executor` | A server process that injects a secret into requests it makes itself | `inject` | Pylota's tool executor for supplier APIs |

| Field | Rule |
|---|---|
| `level` | `platform` (reaches every tenant whose `vault.allowed` is true), `partner` (reaches its partner's tenants) or `tenant` (one tenant). It follows the registering key: a platform key may register either a platform or a tenant-level consumer for a named tenant; a partner key a partner-level consumer or one for one of its tenants; a tenant key one for its own tenant |
| `hosting` | `self_hosted`, or `third_party` with `provider` (1–64 characters, for example `browserbase`) |
| `signing_jwk` | The Ed25519 public key that signs its requests (`kty: OKP`, `crv: Ed25519`); its RFC 7638 thumbprint is the `keyid` |
| `encryption_jwk` | The X25519 public key packets are sealed to (`kty: OKP`, `crv: X25519`) |
| `contract` | `vault-consumer/1`, the version of [§8.5](#85-the-consumer-contract) it accepts |
| `declarations` | Its statements at registration: `session_recording`, `session_logging`, `captcha_solving` (`off`), `extensions` (`none`), `honest_identity` (`web_bot_auth`, `user_agent` or `none`), `observation_paused_during_fill` (`true`), `post_fill_check` (`true`), `modes` (`cdp`, `proxy`) |
| `status` | `pending_activation`, `active`, `suspended` or `revoked` |

A JWK that carries a private member (`d`) is refused (`400 invalid_request`,
`reason = "private_key_supplied"`): private keys are generated where they are used. `idvy vault consumer-keys
generate` writes both key pairs to local files with mode `0600` and prints the public JWKs.

### 8.2 Registration, activation and suspension

| Route | Permission | Rule |
|---|---|---|
| `POST /v1/vault/consumers` | `vault:admin` | Creates it `pending_activation`. At most 10 platform-level, 10 per partner and 5 per tenant that are not `revoked` (`422 vault_consumer_limit_reached`). Audit `consumer.registered` |
| `POST /v1/vault/consumers/{consumer_id}/activate` | `vault:admin` and the activation decision | `Decision::ActivateConsumer`: a platform key; or, for a `tenant`-level, `self_hosted` consumer, a signed-in owner or admin of that tenant after re-authentication; or any key holding `vault:admin` when `PM_CONSOLE` is `off`. Neither `vault.key_approval` nor any other tenant opt-in opens it, so on Pylota Identity Cloud a partner's consumers are activated by the operator, and a `third_party` consumer only by a platform key. `{ "note" }` (1–500 characters) records the review. Audit `consumer.activated` |
| `POST /v1/vault/consumers/{consumer_id}/suspend` | `vault:admin` | Any key that reaches it. Closes its open uses (`consumer_suspended`). Audit `consumer.suspended` |
| `POST /v1/vault/consumers/{consumer_id}/resume` | `vault:admin` and the activation decision | As activation |
| `POST /v1/vault/consumers/{consumer_id}/rotate-keys` | `vault:admin` | `{ "signing_jwk", "encryption_jwk", "overlap_seconds" }` (0–86,400, default 3,600): the new keys take effect at once; the previous signing key still verifies until the overlap ends; every packet is sealed to the encryption key current at redemption, named in `enc_kid`. Audit `consumer.keys_rotated` |
| `DELETE /v1/vault/consumers/{consumer_id}` | `vault:admin` | `revoked`; its public keys are cleared; its open uses close. The row stays for the audit trail |
| `GET /v1/vault/consumers`, `GET /v1/vault/consumers/{consumer_id}` | `vault:admin` | The consumers the key reaches: a platform key every one; a partner key its partner-level consumers and its tenants'; a tenant key its tenant's, and the platform- and partner-level ones that reach it, read-only. Others are `404 vault_consumer_not_found` |

While a partner is `suspended`, its partner-level and tenant-level consumers answer `403 consumer_not_active`
without their status changing ([Security § 4.6](security.md#partner-keys)).

### 8.3 Who reaches what

A consumer redeems a ticket only when the ticket's tenant is within its reach (by level, as above) and the
ticket names that consumer. Both are checked inside the ticket lookup, so a ticket bound to another consumer, or
of a tenant out of reach, gets the same `404 vault_ticket_not_found` as an unknown one.

### 8.4 Authenticating a consumer

Consumer routes take no API key. Each request carries an HTTP message signature (RFC 9421) made with the
consumer's Ed25519 key:

```text
Content-Digest:  sha-256=:<base64 SHA-256 of the body>:                       (RFC 9530; requests with a body)
Signature-Input: sig1=("@method" "@authority" "@path" "content-digest");created=1793000000;expires=1793000030;
                 nonce="<16–64 random bytes, base64>";keyid="<thumbprint of signing_jwk>";alg="ed25519";
                 tag="pylota-identity-vault-consumer"
Signature:       sig1=:<base64 Ed25519 signature over the signature base>:
```

`auth::consumer::verify`, before any other work on the four consumer routes (`Auth::ConsumerSignature` in the
router table; [Security §5.1](security.md#51-deny-by-default-router-table)):

1. Exactly one signature, labelled `sig1`, with `tag = "pylota-identity-vault-consumer"` and `alg = "ed25519"`,
   covering exactly the four components (three for `GET`, without `content-digest`).
2. `keyid` names a consumer's current signing key, or its previous one inside the rotation overlap (one indexed
   D1 read).
3. `created` within 30 seconds of the platform clock either way; `expires` after now and at most 60 seconds
   after `created`.
4. `Content-Digest` equals the SHA-256 of the body as received (bodies of at most 1,572,864 bytes; larger:
   `413 payload_too_large`).
5. The signature base built by `core::httpsig::signature_base`, the RFC 9421 implementation the Web Bot Auth
   path already uses ([Agent signing keys §5](agent-keys.md#5-signed-http-requests-web-bot-auth)), verifies
   under the key with `ed25519-dalek` 3.0.0 `verify_strict`.
6. `INSERT INTO vault_consumer_nonces (consumer_id, nonce, expires_at)` succeeds: a nonce seen before is a
   replay.

Any failure is `401 consumer_unauthenticated`, with a byte-identical body whatever the cause (except
`request_id`), counted in the failed-authentication bucket of `RL_API` per client network
([Security §10](security.md#10-rate-limiting-and-abuse)). Then a consumer that is not `active`, or whose
partner is suspended, gets `403 consumer_not_active`. An `Authorization` header on a consumer route is
`401 consumer_unauthenticated`; a consumer signature on an API route is ignored, and the route answers as it
does without a key.

### 8.5 The consumer contract

Version `vault-consumer/1`. A consumer accepts it at registration. Pylota implements its browser fleet, egress
proxy and executor against it ([§22](#22-pylota-agentic-os)). "Must" is binding; the vault enforces every rule
it can observe (marked **V**) and records the rest as the consumer's declarations in the audit.

**Keys**

- **C1.** The signing and encryption private keys must live only in the consumer component: outside the
  browser process and its renderers, outside the browser container's environment, outside every model's context,
  and outside logs and crash reports. They must be rotated at least every 90 days and at once on suspicion.
- **C2.** The consumer must reach the API host over TLS with no TLS-intercepting proxy in the path (for Pylota:
  not through its own egress proxy).

**Binding and trigger**

- **C3.** It must redeem only a ticket its orchestrator handed it over an authenticated internal channel, never
  a ticket read from a page, a message, a tool argument or model output. **V**: a ticket is bound to one
  consumer.
- **C4.** Before redeeming a fill phase it must determine, from the browser and never from page content: the
  top-level origin; the security origin of the frame holding the focused field (CDP `Page.getFrameTree`); the
  form kind, from field types and `autocomplete` tokens (`current-password` → `login`; `new-password` alone →
  `signup`; both → `password_change`; `one-time-code`, or a short numeric field after a password step →
  `otp`; a password prompt on an already signed-in page → `reauth`); and that the page belongs to the ticket's
  run. It reports them in `context`. **V**: [§8.6](#86-redemption-checks) step 6.
- **C5.** It must never fill into a frame whose origin differs from the top-level page's, nor on a top-level
  origin other than the ticket's. **V**.
- **C6.** The browser that fills must run with session recording, session logging and CAPTCHA solving off and no
  third-party extensions. A third-party hosted browser must have been created with those features off; for
  Browserbase that is `browserSettings.recordSession: false`, `logSession: false` and `solveCaptchas: false`,
  each of which defaults to `true` ([Create a session](https://docs.browserbase.com/reference/api/create-a-session),
  read 2026-10-24), and with no persistent context. It declares these at every redemption. **V**: any other
  value refuses.
- **C7.** The browser must identify itself honestly (for Pylota: its `PylotaBot` user agent and Web Bot Auth
  signatures), with no stealth or fingerprint evasion.
- **C8.** It must apply the integrator's own site policy before redeeming (for Pylota: `browser_automation_targets`,
  where a `prohibited` row anywhere in the domain chain wins).

**Filling**

- **C9.** It must open the packet only in the component that fills, and only after C4 to C8 pass.
- **C10. CDP mode.** From the redemption until the post-fill check ends (at most 30 seconds), it must take no
  screenshot, read no DOM or accessibility tree and run no model step for that tab. It must insert each value
  with `Input.insertText` into the field located by an isolated-world script in the verified frame, never set a
  value through page-world JavaScript, and never pass a value through a Live View input channel.
- **C11. Proxy mode.** The browser types a placeholder the orchestrator gave it (`pidv1-` followed by 26
  Crockford base32 characters, unique to the use and field, not secret). The proxy must substitute the value
  only in the body of one `POST` whose URL origin equals the ticket's origin, within the phase's window, at
  most once per placeholder; drop any request that carries a placeholder to another origin, in a header or a
  URL, or a second time; and remove the value (exact, URL-encoded, base64 and JSON-escaped forms) from the
  response before the browser sees it. When the placeholder does not appear in a request within 30 seconds,
  because the page transforms the value before sending it, the consumer reports `form_not_found`; the
  orchestrator may try again in CDP mode with a new use.
- **C12.** After the attempt, in an isolated world, it must clear every filled field still present, then search
  the DOM text, attribute values, the accessibility tree, the URL and the title for each value and its
  URL-encoded and base64 forms. A match is reported as `value_exposed`; the run stops and is handed to a person.
- **C13.** It must mask password and one-time-code controls in every screenshot and every Live View frame.
- **C14.** It must zeroise packet plaintext after use, and never write a value, packet, ticket or placeholder map
  to a log, trace, crash report, analytics event, evidence bundle, recording or prompt.

**Sessions**

- **C15.** It must load only the state in the packet: cookies through CDP `Storage.setCookies`, keeping
  `httpOnly`, `secure` and `sameSite`, or attached by a proxy to requests for the packet's origins only; local
  storage only for the exact origins, through an isolated world.
- **C16.** At the end of the run it must export state for the item's origins only and write it back within the
  window, and must not write back cookies obtained by solving a challenge. **V**: one `session_load` use per
  item `in_use` at a time; clearance cookies dropped ([§3.6](#36-refused-items-and-values)).

**Secrets**

- **C17.** It must use an `inject` secret only on requests to the ticket's origin with the item's scheme; remove
  the secret (exact, URL-encoded and base64 forms) from every response before a model or a log sees it; and
  discard it at `closes_at`.

**Reports and incidents**

- **C18.** It must report every phase's outcome, including `value_exposed`, `origin_mismatch`,
  `frame_mismatch` and `form_not_found` when nothing was filled ([§8.7](#87-reports)).
- **C19.** On a suspected leak of its keys it must be suspended and its keys rotated; it keeps its own logs,
  without values, for 30 days.
- **C20.** It must pass the consumer conformance fixtures ([§24](#24-tests)) before activation and after each
  release.

### 8.6 Redemption checks

`POST /v1/vault/consumer/redeem`:

```json
{ "ticket": "vtk_7k2m9q4xa0bc_…", "phase": "credentials", "wait_seconds": 0,
  "context": { "run_id": "run_8f2c41", "top_level_origin": "https://tfl.gov.uk",
               "frame_origin": "https://tfl.gov.uk", "form_kind": "login", "trigger": "login_form",
               "mode": "cdp",
               "browser": { "hosting": "self_hosted", "provider": null, "session_recording": "off",
                            "session_logging": "off", "captcha_solving": "off", "extensions": "none" } } }
```

Steps, in order:

1. **Authenticate** the consumer ([§8.4](#84-authenticating-a-consumer)).
2. **Switches.** `PM_VAULT` off, or the `vault_redeem` switch off: `422 vault_disabled`.
3. **Ticket.** Find the use by `lookup`, compare `sha256(ticket)` with `ticket_hash` in constant time, and require
   `consumer_id` = this consumer and the use's tenant within its reach; otherwise `404 vault_ticket_not_found`.
   The tenant's `vault.allowed` is then read: `false` → `422 vault_disabled`.
4. **State.** A closed or expired use: `410 vault_ticket_expired` with `details.reason` = its `close_reason`. An
   unredeemed use past `redeem_by` is expired here (`not_redeemed`).
5. **Phase.** Not one of the use's phases: `400 invalid_request`. Already redeemed: `409 vault_phase_spent`.
   `new_password` before `credentials`: `400 invalid_request`.
6. **Binding**, each a `403 vault_refused` that also closes the use (`refused`), with `details.reason`:

   | Check | `reason` |
   |---|---|
   | `context.run_id` equals the use's | `run_mismatch` |
   | `top_level_origin`, canonicalised, equals the use's origin | `top_level_origin_mismatch` |
   | `frame_origin`, canonicalised, equals the use's origin | `frame_origin_mismatch` |
   | `trigger` equals the use's | `trigger_not_allowed` |
   | `form_kind` fits the phase: `credentials` → `login` or `reauth`, or `signup` while the item's `first_used_at` is `null`; `new_password` → `password_change`; `totp`, `email_otp`, `sms_otp` → `otp`; `session` and `secret` → absent | `form_kind_not_allowed`, `signup_after_first_use` |
   | `session_recording`, `session_logging` and `captcha_solving` are `off`, and `extensions` is `none` | `recording_on`, `logging_on`, `captcha_solving_on`, `extensions_present` |
   | `hosting` and `provider` equal the consumer's registration | `hosting_mismatch` |
   | `mode` is one of the consumer's declared modes, and `proxy` only for an `egress_proxy` | `mode_not_declared` |

7. **Live state.** The item is `active` and its current version is still `ticket_version` (otherwise the use is
   closed `item_rotated` and the answer is `410`); the grant is active; the ledger entry approved; the identity
   active; the tenant not suspended; the consumer active. A failure is the error of [§6.2](#62-creating-a-use)'s
   checks, and closes the use.
8. **Phase work.** `totp`: [§9.1](#91-totp). `email_otp` and `sms_otp`: a long poll of up to `wait_seconds`
   (0–60) for a code ([§9.2](#92-email-codes)); none by then → `200` with `"packet": null, "timed_out": true`,
   and the phase stays open until `closes_at`.
9. **Commit**, one batch guarded by the audit sequence ([§14.2](#142-appending)): the phase marked redeemed; on
   the first phase `status = 'in_use'`, `first_redeemed_at`, `closes_at`; the item's `first_used_at` and
   `last_used_at`; the grant's `use_count`; the audit row `ticket.redeemed` (phase, mode, the hash of `context`);
   the event `vault.ticket.redeemed`. A failed batch returns `503 unavailable` (retryable) and nothing was
   decrypted.
10. **Seal and answer.** Open the DEK with the ticket secret, decrypt only the phase's fields, build the packet,
    seal it with HPKE (the suite of [§4.3](#43-data-keys-and-field-ciphertext)) to the consumer's current
    encryption key with `info = "pylota-identity/vault-packet/v1"` and
    `aad = "{use_id}|{phase}|{consumer_id}|{origin}"`, zeroise, answer `200`.

A phase is spent at the commit. If the response is lost, the consumer cannot redeem it again; the orchestrator
creates a new use, which a `per_run` approval covers within the run. One-use wins over convenience.

```json
{ "use_id": "vus_01JAD6…", "phase": "credentials", "enc_kid": "Xk3H…",
  "packet": { "suite": "x25519-hkdfsha256-aes256gcm", "enc": "…", "ct": "…" },
  "remaining_phases": ["email_otp"], "closes_at": "2026-10-24T08:10:00Z", "timed_out": false }
```

The packet's plaintext:

```json
{ "v": 1, "use_id": "vus_01JAD6…", "phase": "credentials", "origin": "https://tfl.gov.uk",
  "fields": { "username": "compliance.brightwell@pylotamail.com", "password": "…" },
  "not_before": null, "discard_at": "2026-10-24T08:10:00Z" }
```

| Phase | `fields` |
|---|---|
| `credentials` | `username`, `password` |
| `totp` | `code`; `not_before` set when the code is for the next time step ([§9.1](#91-totp)) |
| `email_otp`, `sms_otp` | `kind` (`code` or `link`) and `code` or `link`. A link is opened only in the run's browser and only when its origin is one of the item's origins; otherwise the consumer reports `origin_mismatch` |
| `session` | `state`: `{ cookies, origins }`, already filtered ([§10.1](#101-what-a-session-item-holds)) |
| `secret` | `secret`, `scheme`, `header_name`, `username` |
| `new_password` | `new_password` |

### 8.7 Reports

`POST /v1/vault/consumer/report`, `{ "ticket", "phase"?, "outcome", "final"? }`, once per outcome; a use closed
`window_elapsed` still accepts reports for 10 minutes. Each writes the audit row `ticket.reported` and the event
`vault.ticket.reported`.

| `outcome` | Effect |
|---|---|
| `filled`, `submitted` | None beyond the audit row |
| `signed_in` | The item's health returns to `ok` |
| `signed_up` | `first_used_at` stays set: no further `signup` fills |
| `credentials_rejected`, `otp_rejected` | `health = "needs_attention"` with that reason, `vault.item.health_changed`, and an entry in "Needs a person" |
| `session_rejected` | A session item: `health = "needs_auth"`; the same notices ([§10.4](#104-health-and-hand-off)) |
| `value_exposed` | `health = "needs_rotation"` (`value_exposed`); the use closes; the item refuses uses until a new version is written; `vault_value_exposed_total` pages the operator |
| `origin_mismatch`, `frame_mismatch`, `form_not_found`, `aborted` | Counted in `vault_consumer_reports_total{outcome}`; with `final`, the use closes |
| `password_changed`, `password_change_failed` | [§11.3](#113-rotate_fill-p1) |

With `final: true` the use closes (`completed`), except that a session write-back is still accepted until
`closes_at`.

### 8.8 Write-back

`POST /v1/vault/consumer/write-back`, `{ "ticket", "kind", … }`, within the window:

| `kind` | Allowed for | Body | Effect |
|---|---|---|---|
| `session` | `session_load`, `session_capture`, and `fill` with `capture_into` (after `signed_in`) | `state` | [§10.3](#103-write-back-and-rotation-after-use) |
| `secret` | `inject` on an item with `rotate_after_use: true` | `secret` | A new current version; the previous one is kept for the item's overlap ([§11](#11-rotation)) |
| `password_confirmed` | `rotate_fill` | none | [§11.3](#113-rotate_fill-p1) |

Writing needs only the tenant's public key, so it works in partner custody without an unlock key. Each write-back
is one per use (`409 vault_phase_spent` for a second), and writes `write_back.accepted` or, with its reason,
`write_back.refused`.

`GET /v1/vault/consumer/self` returns the consumer's own record (status, key IDs, level and reach), so a consumer
can check its registration without an API key.

## 9. One-time codes

Codes never pass through an agent or a model. The vault computes TOTP codes itself, and takes email and SMS
codes from the identity's own mailbox and number on the agent's behalf, delivering each into one phase of one
ticket.

### 9.1 TOTP

- The seed is write-only: no route, packet or reveal returns it (a person can see the current code, never the
  seed, [§13.1](#131-human-reveal-p1)).
- At a `totp` redemption the Worker decrypts the seed, computes RFC 6238 with the item's algorithm (SHA1,
  SHA256 or SHA512 over `hmac`, `sha1` and `sha2`, the crates the console's own TOTP uses,
  [Security §7.1](security.md#71-choices)), digits and period, and puts the code in the packet.
- **One code per step per item.** `UPDATE vault_items SET totp_last_step = ?step WHERE id = ?1 AND
  (totp_last_step IS NULL OR totp_last_step < ?step)` runs in the redemption's batch. When it changes no row,
  another use took that step: `409 vault_totp_step_used` (retryable) with `details.retry_after` = the seconds to
  the next step, and the phase stays open. Two runs on one account therefore never submit the same code, which
  many sites reject as a replay.
- **Fresh codes.** When fewer than 7 seconds remain in the current step, the vault issues the next step's code
  with `not_before` = the next step's start, and the consumer waits until then (at most 7 seconds) before typing.
- `core::vault::totp` is tested against the RFC 6238 appendix B vectors for all three algorithms.

### 9.2 Email codes

An identity's code mail is matched to a vault item through the ledger, and from then on only the vault can use
it.

**Quarantine rule 4b** ([Inbound › Quarantine decision](inbound.md#quarantine-decision); staged change), after
rule 4a and before rule 5. A message with a verification match and verdict `pass`, whose matched approved ledger
entry ([Service sign-up ledger §5](service-accounts.md#5-the-verification-gate)) has a linked `active` login item
with `otp_channel.kind = "email"`, `otp_channel.address` equal to the message's `delivered_to`, and the sender's
organisational domain in `otp_channel.sender_domains`, is stored with `status = 'hidden'` and
`quarantine_reason = 'vault_code'`. Its `verifications` row records `account_id` and `vault_item_id`. The
consumer's step-12 query for approved entries also returns each entry's linked items with an email channel
(`IngestInput.approved_accounts[].vault_items`), so the rule reads nothing more in the mailbox. The match uses the
existing rule 4a facts, so a code needs the ledger match even when the tenant's `accounts.require_approval` is
`false`: rule 4b looks the entry up whenever the identity has an item with an email channel.

What `hidden` with `vault_code` means:

- The message is invisible to every agent-facing surface: lists, threads, search, `wait`, the event feed, webhooks
  (no `message.received` and no `verification.received`), MCP and triage, as every `hidden` message is
  ([Security §5.3](security.md#53-cross-level-read-access)).
- Unlike other hidden mail, its content is withheld from API keys **whatever their permissions**: a list with
  `status=hidden` from a key holding `quarantine:review` shows the row with `subject`, `text`, `html`,
  `extracted_text` and attachments `null` and `content_withheld: "vault_code"`; `GET …/raw` answers
  `404 message_not_found`. A signed-in person sees it whole in the console.
- It is never released (`409 not_quarantined`, as for every hidden message) and never counted in notifications.
- The message, its raw object and its `verifications` row are deleted 24 hours after `received_at` by the
  mailbox's daily maintenance (the `verifications` retention of [Privacy §2](privacy.md#2-data-inventory)).

**Delivering a code** (an `email_otp` phase, [§8.6](#86-redemption-checks) step 8). Each second until
`wait_seconds` pass, the Worker sends `MailboxRequest::VaultTakeCode { item_id, address, sender_domains,
since_ms: use.created_at − 60 000, use_id }` to the identity's mailbox. In one transaction the mailbox selects the
oldest `verifications` row with that `vault_item_id`, `received_at ≥ since_ms`, `expires_at > now` and
`vault_consumed_at IS NULL`, sets `vault_consumed_at` and `vault_use_id`, and returns the kind and value. The
code is then spent even if the redemption's batch fails afterwards: fail closed. A code that arrives with no
open `email_otp` phase stays hidden and expires unused.

**A strict mode for the whole tenant.** The guard field `vault.codes_into_fills_only` (default `false`) makes rule
4b apply to **every** message with a verification match, whatever its verdict, sender or ledger entry, except
senders on the tenant's receive-allow list: all such mail is stored `hidden` with `vault_code`, a code is usable
only by a vault phase whose item matches as above, and a person sees the rest in the console.
`wait` with `kind=verification` then answers `403 policy_denied` with `details.reason =
"codes_into_fills_only"`. Turning it on is a tightening any workspace writer may make; turning it off loosens a
guard field ([Workspace policy §3](workspace-policy.md#3-decisions-reserved-for-people)).

`wait` and `mail_wait` are otherwise unchanged: a ledger entry without a vault item releases its codes to the
agent as today. The difference is deliberate and visible: the guide tells integrators to give every account an
item with an email channel, or to turn on the strict mode.

### 9.3 SMS codes

The same model, with the phone design ([Phone numbers](phone-numbers.md), written in parallel) providing the
channel. The vault needs from it:

| Need | Rule |
|---|---|
| A code source | `trait SmsCodeSource { fn take(&self, tenant_id, number_id, senders, since_ms, use_id) -> Option<OneTimeCode> }`, consuming a code once, as `VaultTakeCode` does |
| Withholding | An inbound SMS to a number named by an active login item's `otp_channel` (`kind = "phone"`), whose sender matches one of `otp_channel.senders` and whose body has a verification match (`core::classify::find_verification`, the email rule), is withheld from every agent-facing surface and kept at most 24 hours |
| Strict mode | `vault.codes_into_fills_only` withholds every SMS with a verification match on the tenant's numbers |
| Erasure | The phone design's erasure covers the withheld messages and code rows |

**SMS has no sender authentication.** An alphanumeric sender ID or a number can be spoofed, and nothing like
DMARC proves who sent a text. A matched sender is a filter, not proof. The vault therefore delivers an SMS code
only into an `sms_otp` phase that is open, for a use created at most 10 minutes earlier, and never treats an
unsolicited SMS code as usable. `sms_otp` phases are P1 and ship with the phone milestones; until then a use
naming `sms_otp` is `400 invalid_request` (`reason = "sms_not_available"`). The interface is reconciled with the
phone design at integration.

## 10. Sessions

### 10.1 What a session item holds

A session item replaces Pylota's `browser_sealed_sessions`: cookies and web storage captured after a sign-in, for
the item's origins only.

- **Shape.** `{ "cookies": [{ "name", "value", "domain", "path", "expires", "httpOnly", "secure", "sameSite" }],
  "origins": [{ "origin", "local_storage": [{ "name", "value" }] }] }`, the shape of Playwright's
  `storageState`. IndexedDB and service workers are not kept.
- **Filter** (`core::vault::cookies::filter`), applied at create, import and every write-back: a cookie is kept
  when its domain, without a leading dot, equals one of the item's origin hosts, or is a parent of one down to
  and including its registrable domain (`.tfl.gov.uk` for `https://tfl.gov.uk` is kept; `.gov.uk`, a public
  suffix, is not); every other cookie (an identity provider's, an advertiser's) is dropped. Cookies named in the
  clearance list are dropped ([§3.6](#36-refused-items-and-values)). Local storage is kept only for the exact
  origins. The response and the audit row count what was dropped (`cookies_dropped`,
  `clearance_cookies_dropped`, `origins_dropped`).
- **Bounds.** At most 1,048,576 bytes of filtered JSON, 500 cookies and 10 origins
  (`422 vault_session_too_large`, `details.limit`).
- **Lifetime.** `ttl_seconds`, 3,600 to `vault.session_max_ttl_days` (default 30 days, a lower-only field),
  default 7 days. Each version's `expires_at` is its write time plus the item's TTL, so a refreshed session gets
  a new expiry.
- **Device-bound sessions.** Sites that adopt Device Bound Session Credentials bind refresh to a device key,
  so "exfiltrated cookies quickly expire" (Google, 2026-04-09, read for the browser study on 2026-10-24); a
  replayed session then stops working early. The vault treats every session as short-lived: a rejected session
  makes the item `needs_auth`, and a `fill` with `capture_into` recovers it where a login item exists.

### 10.2 Loading

A `session_load` use has one `session` phase. The packet carries the filtered state; the consumer loads it under
C15 and runs. Only one `session_load` use per item may be `in_use` at a time (`403 vault_refused`,
`session_in_use`), so two runs never share one sign-in.

### 10.3 Write-back and rotation after use

Before `closes_at`, the consumer writes back the state it holds at the end of the run (C16). The Worker:

1. filters it ([§10.1](#101-what-a-session-item-holds)) and checks the bounds;
2. creates version `n + 1` with a new DEK sealed to the tenant's public key, and writes its ciphertext to R2 at
   `t/{tenant_id}/vault/{item_id}/{n+1}`;
3. in one batch guarded by the audit sequence: inserts the version as `current`; deletes version `n`'s row (its
   DEK and field ciphertext); points the item at `n + 1`; closes any other open use of the item
   (`item_rotated`); writes `write_back.accepted` and `item.rotated` with `vault.item.rotated`;
4. after the commit, deletes version `n`'s R2 object; a failed delete is retried by the daily vault sweep, which
   deletes any `t/{tenant_id}/vault/{item_id}/{v}` object without a version row.

A batch that fails deletes the new object. The previous session is gone the moment the new one is current, so a
copy of an old jar never outlives its replacement in the vault. A consumer that writes nothing back leaves the
current version in place until it expires.

`session_capture` uses have no phase: they exist so that a person's hand-off sign-in can be stored. A `fill` with
`capture_into` writes its session into the named session item the same way, after a `signed_in` report.

### 10.4 Health and hand-off

| Event | Item | Notice |
|---|---|---|
| A consumer reports `session_rejected` | `health = "needs_auth"` (`session_rejected`); `session_load` refused until a new version is written | `vault.item.health_changed`; "Needs a person" in the console's Overview and the daily email (a count and the item's title, [Notifications](notifications.md)) |
| A version passes `expires_at` | `health = "needs_auth"` (`session_expired`); the daily vault sweep deletes the expired version's row and object | The same |
| A new version is written | `health = "ok"` | `vault.item.health_changed` |

Signing in again is a hand-off to a person: the integrator opens its own human takeover (for Pylota, Live View),
the person signs in, optionally with a vault `fill` they approve on their phone (`trigger = "person_handoff"`),
and the integrator's consumer writes the result back through a `session_capture` use, which always needs a
person's approval (the person doing the hand-off can approve it in the same flow).

### 10.5 Importing an existing session

`POST /v1/identities/{identity_id}/vault/items` with `kind: "session"` accepts
`"import": { "source": "pylota_sealed_session", "source_ref": "<opaque, ≤ 64 characters>" }` beside `state`. The
state is filtered as any other; `source` and `source_ref` are kept in the item's audit row so Pylota can match
its migration ([§22.3](#223-migrating-from-browser_sealed_sessions)). It is otherwise an ordinary create.

## 11. Rotation

### 11.1 Versions

An item has at most three version rows: `current`, `previous` (kept for an overlap) and `pending` (a
`rotate_fill` in progress). Older versions are deleted when they are superseded: there is no history beyond
`previous`.

| Kind | Previous version kept | Used by |
|---|---|---|
| `login` | `overlap_hours` given on rotation: 0–168, default 24 | A `fill` use that sets `"version": "previous"`, while the overlap lasts, for a site that has not yet taken the new password |
| `api_key` | The same, default 24 | An `inject` use with `"version": "previous"` (dual-phase rotation, as AWS staging labels and Infisical do) |
| `session` | None: a write-back deletes the previous version at once | — |
| `managed_secret` | None | — |

The every-minute vault cron deletes `previous` rows past their overlap (row and DEK).

### 11.2 Rotating by hand

`POST /v1/vault/items/{item_id}/versions` (`vault:write`; an identity key only for its own identity's items),
with the kind's values (a `password` or `{ "generate": … }`, a `totp`, a `secret`, a `state`) and
`overlap_hours`. Fields not given are carried from the current version (re-sealed under the new DEK: this needs
the tenant key, so in partner custody a rotation that carries fields needs the unlock key; one that replaces every
field does not). It writes the new version, makes it `current`, makes the old one `previous` or deletes it, closes
open uses of the item (`item_rotated`), sets `rotation.last_rotated_at`, and writes `item.rotated`. Changing a
value in the vault does not change it at the service: for a login, the guide says to change the password at the
site first, or to use `rotate_fill`.

### 11.3 `rotate_fill` (P1)

An automated password change at the site, always approved by a person for each use:

1. A `rotate_fill` use (`trigger = "password_change"`, effective level `per_use`). At minting, the vault generates
   the new password (the item's generation rule) and stores it as a `pending` version.
2. The consumer redeems `credentials` (the current password) and `new_password`, on a page whose `form_kind` is
   `password_change`, fills both and submits.
3. It reports `password_changed` and writes back `password_confirmed`: the pending version becomes `current`, the
   old one `previous` with a 24-hour overlap. Or it reports `password_change_failed`: the pending version is
   deleted and nothing changes.
4. A pending version not confirmed by `closes_at` stays `pending` for 24 hours, flagged
   `health = "needs_attention"` (`rotation_unconfirmed`), because the site may or may not have taken it; a person
   resolves it in the console by keeping either version.

This is the only operation that fills a password-change page. The consumer contract otherwise hands
password-change, security-settings and recovery pages to a person.

### 11.4 Rotation after use, and reminders

- **Sessions** rotate after every use that writes back ([§10.3](#103-write-back-and-rotation-after-use)).
- **API keys** rotate after use where the service supports it: an item with `rotate_after_use: true` lets the
  `executor` or `egress_proxy` consumer, after using the key, call the service's own rotation endpoint itself and
  write the new secret back (`kind = "secret"`). The vault never calls a service.
- **Logins** rotate through `rotate_fill`.
- **Reminders.** An item with `rotation.remind_after_days` gets `health = "needs_rotation"` (`rotation_due`) when
  that many days pass since `last_rotated_at` (or creation), with `vault.item.health_changed`. It stays usable.
- **After an incident**, a value reported exposed blocks the item until it is rotated
  ([§8.7](#87-reports)).

## 12. Signing

P1. An `api_key` item with a `sign` scheme lets an identity get a signature made with the secret, without the
secret:

| Scheme | Request (`sign_request` in the use) | Returned |
|---|---|---|
| `hmac_sha256` | `message_base64`: 1–65,536 bytes | The MAC in the item's encoding |
| `aws_sigv4` | `method`, `url` (its origin must be one of the item's origins), `headers` (at most 50), `body_sha256`, `timestamp` (within 5 minutes of now) | `{ "authorization", "x_amz_date", "x_amz_content_sha256" }` |

A `sign` use goes through grants and approval like any other, has no ticket and no consumer, and is answered by
`POST /v1/identities/{identity_id}/vault/uses` itself when it needs no approval, or by
`POST /v1/vault/uses/{use_id}/signature` once approved (once only; then the use closes). In partner custody both
need the unlock key. The MCP tool `vault_sign` maps to them. A signature over caller-chosen bytes is a capability
too, which is why it needs a grant and follows the item's approval level; the result is not a secret, so it may
reach a model. Both schemes are tested against their published vectors (RFC 4231 for HMAC-SHA256 and the AWS
SigV4 test suite, the vectors behind [Security §7.1](security.md#71-choices)'s SES signing).

## 13. Human reveal and managed secrets

### 13.1 Human reveal (P1)

A person sometimes needs a value: to sign in themselves, or to read a port-out PIN to another carrier. Reveal
exists only in the console:

- **Who**: an owner or admin of the workspace, signed in within the last 10 minutes
  ([Console › Re-authentication](console.md#re-authentication)), at most 10 reveals per person per UTC day.
- **What**: a login's username, password and recovery codes and its current TOTP code (never the seed); an API
  key's secret; a managed secret. Never a session's cookies.
- **How**: a `POST` form shows the value once on a page served with `Cache-Control: no-store` and no other
  content from the item; reloading shows nothing.
- **Record**: audit `item.revealed` with `actor_user_id`, and the event `vault.item.revealed`.
- **Partner custody**: not available (`vault_unlock_required`): the console never holds an unlock key.
- **No API, MCP or CLI route reveals**, for any key, platform keys included. An integrator that needs a person to
  see a value sends them to the console.

### 13.2 Managed secrets

`managed_secret` items hold secrets the deployment itself needs to keep for a resource it manages, starting with
the phone design's port-out PIN: a random PIN set on each number at Telnyx as `external_pin`, checked when another
carrier asks to port the number away (Telnyx research, read 2026-10-24). The vault offers the phone service an
internal interface, not a route:

```rust
// crates/worker/src/vault/managed.rs
pub enum ManagedPurpose { PortOutPin }
pub async fn put_managed(ctx: &VaultCtx, tenant_id: &str, identity_id: Option<&str>,
                         owner_ref: &str, purpose: ManagedPurpose, value: Zeroizing<String>) -> Result<ItemId>;
pub async fn use_managed(ctx: &VaultCtx, tenant_id: &str, owner_ref: &str, purpose: ManagedPurpose,
                         f: impl FnOnce(&str) -> Result<()>) -> Result<()>;     // audited as item.used_internal
pub async fn delete_managed(ctx: &VaultCtx, tenant_id: &str, owner_ref: &str, purpose: ManagedPurpose) -> Result<()>;
```

- Created, rotated and deleted only by the phone service (`owner_ref` is the phone design's number ID), with
  `managed_by: "phone"`. API routes list them with their metadata (`vault:read`) and refuse every write, grant
  and use (`422 invalid_request`, `reason = "managed_item"`).
- `use_managed` lends the value to a closure inside the Worker (for example to re-apply the PIN at Telnyx) and
  zeroises it; it needs the tenant key, so in partner custody the phone service cannot re-apply a PIN without the
  partner's unlock key, and the phone design must say what it does then.
- A person reveals the PIN in the console ([§13.1](#131-human-reveal-p1)) when they port the number out
  legitimately.
- Deleted with the number (release, port-out completed) and by erasure.

The interface is reconciled with the phone design's request at integration.

## 14. Audit and events

### 14.1 The audit row

`vault_audit` is append-only: no route or job updates a row; rows are deleted only by retention and erasure
([§16](#16-retention-and-erasure)).

| Column | Holds |
|---|---|
| `id` | `vad_` + ULID |
| `tenant_id`, `seq` | The tenant and a sequence number, contiguous per tenant from 1 |
| `prev_hash`, `row_hash` | `row_hash = hex(sha256(canonical_json(every other column)))`; `prev_hash` = the previous row's `row_hash`, or the tenant's anchor ([§16.1](#161-retention)), or 64 zeros for the first row |
| `action` | One of the actions below |
| `item_id`, `version`, `grant_id`, `use_id`, `consumer_id`, `identity_id` | The objects concerned |
| `actor_key_id`, `actor_user_id`, `actor_consumer_id`, `on_behalf_of` | Who acted: a key, a person, a consumer, or the system (all `NULL`) |
| `operation`, `phase`, `outcome`, `reason` | Enumerations only |
| `site` | The registrable domain of the origin concerned, never a URL |
| `run_id` | The use's run identifier |
| `details_json` | Counts and enumerations: `mode`, `context_sha256`, `cookies_dropped`, `direct`, `close_reason`, `custody` |
| `created_at` | Platform clock |

Never in a row: a value, a ticket, an unlock key, a packet, a username, a title, a purpose, a note, an address or
a full URL. `core::vault::audit::Row` has typed fields only, as `worker::log` does
([Security §12](security.md#12-logging-rules)).

Actions: `item.created`, `item.updated`, `item.rotated`, `item.archived`, `item.restored`, `item.deleted`,
`item.revealed`, `item.used_internal`, `item.health_changed`; `grant.requested`, `grant.approved`, `grant.denied`,
`grant.revoked`, `grant.expired`; `use.requested`, `use.approval_requested`, `use.approved`, `use.denied`,
`use.expired`, `use.canceled`, `use.closed`; `ticket.issued`, `ticket.redeemed`, `ticket.refused`,
`ticket.reported`; `write_back.accepted`, `write_back.refused`; `consumer.registered`, `consumer.activated`,
`consumer.suspended`, `consumer.resumed`, `consumer.keys_rotated`, `consumer.revoked`; `vault.key_rotated`,
`vault.custody_changed`, `vault.revoke_all`. Refusals (`ticket.refused`, and every `403 vault_refused` and
`vault_not_granted` at use creation, written as `use.requested` with `outcome = "refused"`) are their own rows,
counted apart from errors.

### 14.2 Appending

Every row is written in the D1 batch of the change it records, so a change never commits without its row and a
row never exists for a change that did not commit:

1. Read the tenant's head: `SELECT seq, row_hash FROM vault_audit WHERE tenant_id = ?1 ORDER BY seq DESC
   LIMIT 1` (or the anchor).
2. Build the row with `seq = head + 1` and `prev_hash = head hash`; compute `row_hash`.
3. Guard **every** statement of the batch with `WHERE NOT EXISTS (SELECT 1 FROM vault_audit WHERE tenant_id = ?1
   AND seq = ?new_seq)`, and insert the row last, under the unique index `(tenant_id, seq)`.
4. When the insert changed no row, another write took the sequence and every statement was a no-op: repeat from
   step 1, at most three times in all, then `503 unavailable` (retryable). The change and its row commit
   together or not at all.

A refusal writes its row in a batch of its own (it changes nothing else). If that batch fails, the request still
answers with its refusal; the miss is counted in `vault_audit_append_failures_total`, which pages.

### 14.3 Who reads it

- People: the console's vault audit page (owners and admins), filtered by item, identity, consumer and action,
  with a "verify chain" button that recomputes the hashes over the page shown.
- API: `GET /v1/tenants/{tenant_id}/vault/audit` (`audit:read`; platform, partner or tenant key), cursor
  pagination in `seq` order, filters `item_id`, `identity_id`, `consumer_id`, `action`, `since_seq`. Each entry
  carries `seq`, `prev_hash` and `row_hash`, so a reader can verify the chain from any point it stored.
- The event feed: [§14.4](#144-events).

### 14.4 Events

Each audit row except the refusals of a consumer's failed authentication emits one event, written to
`event_index` in the same batch: platform events with `tenant_id` set and `identity_id` set where the row has one
([Webhooks › Platform events](webhooks.md#platform-events)), delivered to the tenant's, its partner's and platform
endpoints and appended to the tenant's log. Every `data` carries `audit: { "seq", "row_hash" }`, so a partner that
stores its events holds an independent copy of the chain heads and can detect a rewritten history.

| Event | `data` (besides `audit`) |
|---|---|
| `vault.item.created`, `vault.item.updated`, `vault.item.rotated`, `vault.item.deleted`, `vault.item.revealed` | `item` (the item as the API returns it; for `deleted`, `item_id` and the shred receipt) |
| `vault.item.health_changed` | `item_id`, `health`, `health_reason` |
| `vault.grant.requested`, `vault.grant.approved`, `vault.grant.denied`, `vault.grant.revoked`, `vault.grant.expired` | `grant` |
| `vault.use.requested`, `vault.use.approval_requested`, `vault.use.approved`, `vault.use.denied`, `vault.use.expired`, `vault.use.canceled`, `vault.use.closed` | `use` (never the ticket) |
| `vault.ticket.issued`, `vault.ticket.redeemed`, `vault.ticket.refused`, `vault.ticket.reported` | `use_id`, `item_id`, `consumer_id`, `phase`, `outcome` or `reason` |
| `vault.consumer.changed` | `consumer_id`, `change` (`registered`, `activated`, `suspended`, `resumed`, `keys_rotated`, `revoked`) |
| `vault.key.changed` | `change` (`rotated`, `custody_changed`, `revoke_all`), `custody` |

Twenty-four types. Visibility in the feed ([Event feed §4](event-feed.md#4-who-sees-which-events); staged rows):
`vault.item.*`, `vault.grant.*`, `vault.use.*` and `vault.ticket.*` need `vault:read`, and identity keys see those
of their own identity; `vault.consumer.changed` and `vault.key.changed` need `vault:admin` and are never shown to
identity keys. An endpoint subscribing to them needs the same permissions to be created (the J29 rule). No event
carries a value, a ticket, a username, a title or a purpose beyond the item object (whose `title` and `username`
are not secrets; an integrator that wants neither in its webhooks filters by type).

## 15. Kill switches and incident response

| Switch | Who | Effect | How fast |
|---|---|---|---|
| `PM_VAULT = "off"` | The operator (`idvy wrangler secret put`, a new version) | Every vault route and consumer route answers `422 vault_disabled`. Hidden `vault_code` mail stays hidden: turning the vault off never releases codes | The next request on the new version |
| Ops switch `vault_redeem` | A platform key (`idvy ops switch vault_redeem off`) | Every redemption, write-back and ticket mint refuses (`422 vault_disabled`); reads and approvals continue | Read from `platform_state` on each request, as the other switches are ([Observability §5.6](observability.md#56-automatic-containment)) |
| `vault.allowed: false` on a tenant | A platform key (platform-only field) | That tenant's vault routes refuse and its open uses close | The next request |
| `POST /v1/tenants/{tenant_id}/vault/revoke-all` | `vault:admin` | `{ "shred_sessions": bool, "archive_items": bool }`: revokes every grant, closes every open use, optionally deletes every session version (items stay, `needs_auth`) and archives every item. Audit `vault.revoke_all` | One batch per 1,000 rows, in the request |
| Partner suspension | A platform key | Its keys and its consumers refuse | The next request |
| Identity pause | As today | Uses and redemptions for it refuse; its open uses close | The next request |
| Consumer suspend or revoke | `vault:admin` | Its redemptions refuse; its open uses close | The next request |
| Item archive or delete; grant revoke | As above | That item or grant only | The next request |

**After a suspected leak**, the runbook (`docs/src/project/design/observability.md`, staged entry "Vault
incident"):

| Suspected | Do |
|---|---|
| A consumer's keys | Suspend it; rotate its keys; review its `ticket.redeemed` rows since the suspected time; tickets are useless without the orchestrator's live tickets, but treat the items it redeemed as exposed and rotate them |
| An orchestrator key with `vault:use` | Revoke the key; uses still need grants, approvals and a consumer, so review `use.*` rows by that key |
| A value seen by a model or in a log | Report `value_exposed` (or set the item to `needs_rotation` by hand), rotate the value at the site, then in the vault |
| `PM_MASTER_KEY` and D1 together | Rotate the master key; in operator custody every vault value must be treated as exposed: `revoke-all` with `shred_sessions`, then rotate every login and API key at its service. Partner-custody tenants are unaffected unless an unlock key also leaked |
| A partner unlock key | Enrol a new custody slot, release and re-enrol to drop the leaked one, and rotate values |

## 16. Retention and erasure

### 16.1 Retention

| What | Kept | Removed by |
|---|---|---|
| Items, current versions | Until deleted by a person or erasure | — |
| `previous` versions | Their overlap | The every-minute vault cron |
| Session versions | Until `expires_at` | The daily vault sweep (row, then R2 object) |
| Grants: `denied`, `revoked`, `expired` | 90 days after the decision | The global retention job's new `vault` step |
| Uses: closed, `denied`, `expired`, `canceled` | 90 days after they ended; their ticket hash and sealed DEK are cleared when they end | The same step |
| `vault_consumer_nonces` | Until 60 seconds after the signature's `expires` | The every-minute vault cron |
| `vault_audit` | 400 days | The same step; before deleting the oldest rows of a tenant it stores the last deleted row's `seq` and `row_hash` in `vault_audit_anchors`, so the chain stays verifiable from the anchor |
| Revoked consumers | 400 days after revocation, for the audit trail | The same step |
| Hidden `vault_code` mail and its `verifications` rows | 24 hours | Mailbox maintenance |

### 16.2 Deleting an item

`DELETE /v1/vault/items/{item_id}` (`vault:admin`; never an identity key) is a crypto-shred with a receipt, in
one batch guarded by the audit sequence: every version row (each DEK and field ciphertext) deleted, the item's
grants deleted, its open uses closed (`item_deleted`) with their sealed DEKs cleared, the item row deleted, the
audit row `item.deleted` and the event. After the commit, every R2 object under
`t/{tenant_id}/vault/{item_id}/` is deleted (retried by the daily sweep). Response `200`:

```json
{ "item_id": "vit_01JAD4…", "deleted": true, "versions_shredded": 2, "r2_objects_deleted": 1,
  "uses_closed": 0, "grants_deleted": 1, "restorable_until": "2026-11-23T09:00:00Z",
  "custody": "operator" }
```

`restorable_until` is 30 days ahead: a D1 Time Travel restore to an earlier point would bring the item back
([D1 limits](https://developers.cloudflare.com/d1/platform/limits/), read 2026-10-24), until the erasure
re-application that follows every restore deletes it again ([Privacy §11](privacy.md#11-what-remains-after-deletion)).
In partner custody the field is `null` once the partner has discarded its unlock keys, because restored rows
cannot be opened; the vault cannot know whether it has, so the guide explains it.

Deleting a session item does not sign it out at the site: a copy stolen earlier stays valid there until the site
expires it. The guide tells integrators to use the site's "sign out other sessions" through a person when they
suspect a leak.

### 16.3 Erasure scopes

| Scope | Vault rows | Where |
|---|---|---|
| Identity ([Privacy §6.5](privacy.md#65-identity-scope-fr-idn-4)) | Its identity-scoped items with their versions, grants and uses deleted as in [§16.2](#162-deleting-an-item); its grants on tenant-scoped items deleted and their uses closed; R2 objects of its items deleted by key; audit rows kept (IDs only), with their `run_id` cleared | Step `scrub_control_plane`, before `service_accounts` |
| Tenant ([Privacy §6.6](privacy.md#66-tenant-scope)) | First `vault_custody_slots` and `vault_keys` (every tenant key: the crypto-shred), then `vault_uses`, `vault_grants`, `vault_item_versions`, `vault_items`, tenant-level `vault_consumers`, and `vault_audit` and `vault_audit_anchors` except the `vault.revoke_all` row the erasure itself writes; `t/{tenant_id}/vault/` swept with the rest of the prefix | Step `stop_routing` closes open uses; step `delete_d1_rows` deletes the rows in that order; step `sweep_r2` |
| Counterparty, message, thread | Not affected: vault items hold the agent's and the operator's own credentials, not a counterparty's. A hidden `vault_code` message is erased with its message like any other | — |
| Person (console account) | Audit rows keep `actor_user_id` as an opaque ID, as audit rows do today | — |

Receipts gain `vault_items_deleted`, `vault_versions_shredded`, `vault_keys_shredded` (tenant scope),
`vault_uses_deleted` and `vault_audit_deleted` ([Privacy §10](privacy.md#10-receipt); staged change). The probe
step adds: no `vault_items`, `vault_item_versions`, `vault_keys` or `vault_custody_slots` row of the scope remains,
and `t/{tenant_id}/vault/` lists empty for a tenant scope.

<!-- PART4 -->
