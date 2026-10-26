Where this goes: `docs/src/project/prd.md`. Six insertions, each introduced below by the heading or row it follows.

## 1. In "## 5. Scope and priorities", add this row to the table after the row "Phone numbers" (or after "Plans and billing" if that row is not yet pasted)

| Vault (behind `PM_VAULT`, [ADR 0021](adr/0021-agent-vault.md)) | `login`, `api_key` and `session` items bound to approved ledger entries and exact origins; write-only values sealed per version under a per-tenant key; grants and uses approved by people, bound to a digest; one-use tickets redeemed by registered consumers with signed requests and sealed packets; the consumer contract and conformance fixtures; TOTP and email codes into fills; sessions with filtering, write-back and expiry; hash-chained audit; kill switches; crypto-shred erasure; the six MCP tools and `idvy vault`; the import route for Pylota's sessions (FR-VLT-1–6, 8–13, 15, 17, 21–26, 28) | Partner custody (FR-VLT-7); SMS codes into fills with the phone milestones (FR-VLT-16); TOTP (FR-VLT-14); rotation by hand, `rotate_fill` and rotate-after-use (FR-VLT-18); signing (FR-VLT-19); console reveal (FR-VLT-20); MCP and CLI tooling (FR-VLT-27); the session import route (FR-VLT-29); outbound ranges by embedded IPv4 in the fixtures (FR-VLT-30) | Credentialed fetch through the vault; share links to people; breach checks of stored passwords; SSH keys and documents |

## 2. In "### Non-goals (v1.0)", add these bullets after the last bullet about phone numbers (or after "Attachments fetched from URLs.")

- Showing a vault value to an agent, a model or an API key, in any form. Agents use credentials through
  registered consumers and never see them ([Vault](design/vault.md)).
- Payment cards, bank details and passkeys in the vault; CAPTCHA solving; storing bot-challenge clearance.
- A zero-knowledge or end-to-end encrypted vault. Pylota Identity Cloud holds the keys that decrypt the vault,
  and says so ([Vault § 1.2](design/vault.md#12-the-trust-model)).
- Credentialed fetch: the vault does not make outbound requests with a secret on an agent's behalf in v1.0.

## 3. In "## 6. Functional requirements", add this subsection after "### 6.23 Phone numbers" (after FR-PHN-42)

### 6.24 Vault

Behind `PM_VAULT` ([ADR 0021](adr/0021-agent-vault.md)). Priorities in brackets; acceptance names rows of
section U of the [edge-case register](edge-cases.md).

- **FR-VLT-1** (P0) The vault **must** be off unless `PM_VAULT` is `on` and the tenant's `vault.allowed` is
  `true`; `vault.allowed` is set only by a platform key; turning the vault off **must not** release a withheld
  code. Acceptance: U1, U2, U70.
- **FR-VLT-2** (P0) No route, MCP tool, event, log, audit row, idempotency record or export **may** return or
  contain a vault value, ticket, packet, unlock key or TOTP seed; value-holding types **must not** implement
  `Debug` or `Serialize`. Acceptance: U3, U4, U5.
- **FR-VLT-3** (P0) Items **must** be one of `login`, `api_key` or `session` (and the internal
  `managed_secret`), bound to 1–10 exact `https` origins canonicalised as specified; wildcards, IP literals,
  internal names and confusable hosts **must** be refused. Acceptance: U6, U7, U8, U85.
- **FR-VLT-4** (P0) The vault **must** refuse payment cards, bank details, passkeys and WebAuthn keys, and
  **must** drop bot-challenge clearance cookies. Acceptance: U9, U10, U11.
- **FR-VLT-5** (P0) Every usable item **must** be bound to an approved service-ledger entry whose registrable
  domain (or `login_domains`) covers each origin, re-checked at grant approval, use creation and redemption;
  closing the entry **must** archive the item and close its uses. Acceptance: U12, U13, U14, U15.
- **FR-VLT-6** (P0) Values **must** be sealed per item version under a fresh DEK, with AAD binding tenant, item,
  version and purpose, the DEK sealed by HPKE to the tenant's X25519 key, and the tenant key sealed under the
  master key (and, in partner custody, under the partner's unlock key). Acceptance: U16, U17, U18.
- **FR-VLT-7** (P1) A tenant's own partner key **must** be able to move the tenant into partner custody and back,
  with one or two unlock keys shown once; unwrapping routes **must** require the unlock key in that mode.
  Acceptance: U19, U20, U21.
- **FR-VLT-8** (P0) An agent **must** obtain use of an item only through a grant a person approves (or a
  platform or partner key under `vault.key_approval` with `on_behalf_of`); the requesting key **must never**
  approve; identity keys **must never** hold `vault:use`, `vault:approve` or `vault:admin`; a later change to an
  item **must not** widen a live grant. Acceptance: U22, U23, U24, U78, U79.
- **FR-VLT-9** (P0) Uses **must** be created only by `vault:use` keys from an enumerated trigger, with the
  approval level computed as the strictest of item, tenant floor and operation floor, and approval bound to the
  use's digest. Acceptance: U25, U26, U27, U28.
- **FR-VLT-10** (P0) Tickets **must** be one-use per phase, bound to one consumer, the run, the origin and a
  60-second first-redemption window, stored only as a hash, and closed early by every cause listed.
  Acceptance: U29, U30, U31, U32.
- **FR-VLT-11** (P0) Consumers **must** authenticate every request with an RFC 9421 Ed25519 signature with nonce
  replay protection, be activated by a person or platform key, and receive values only HPKE-sealed to their
  current encryption key. Acceptance: U33, U34, U35, U36.
- **FR-VLT-12** (P0) Redemption **must** refuse and close the use on any mismatch of run, top-level origin,
  frame origin, trigger, form kind, browser declarations, hosting or mode, and on any live-state change.
  Acceptance: U37, U38, U39.
- **FR-VLT-13** (P0) The consumer contract `vault-consumer/1` (C1–C21) **must** be published, versioned and
  tested by conformance fixtures that a consumer passes before activation. Acceptance: U40, U73–U77, U80, U82.
- **FR-VLT-14** (P1) TOTP codes **must** be computed inside the vault, one code per step per item, with the seed
  never returned. Acceptance: U41, U42.
- **FR-VLT-15** (P0) Verification email for a vault login item's channel **must** be withheld from every agent
  surface and API key and delivered only into an open `email_otp` phase, once; a strict mode **must** extend
  this to every verification mail. Acceptance: U43, U44, U45, U46.
- **FR-VLT-16** (P1) SMS codes **must** follow FR-VLT-15 through the phone design's code source, only into an
  `sms_otp` phase of a use created at most 10 minutes before. Acceptance: U47, U48.
- **FR-VLT-17** (P0) Session items **must** be filtered to their origins, bounded, loaded by one run at a time,
  rotated at write-back with the previous version deleted, and expired by TTL. Acceptance: U49, U50, U51, U52.
- **FR-VLT-18** (P1) Items **must** keep at most `current`, `previous` and `pending` versions; rotation by hand,
  `rotate_fill` and write-back of rotated API keys **must** close open uses. Acceptance: U53, U54, U55.
- **FR-VLT-19** (P1) `api_key` items with a sign scheme **must** produce HMAC-SHA256 and AWS SigV4 signatures
  through grants and approval, without returning the secret. Acceptance: U56.
- **FR-VLT-20** (P1) A person **must** be able to reveal a value only in the console, re-authenticated, at most
  10 times a day, never a TOTP seed or session cookies; no API, MCP or CLI route **may** reveal. Acceptance:
  U57, U58.
- **FR-VLT-21** (P0) The vault **must** keep internal managed secrets for other services (the port-out PIN),
  writable only by their owner service, with rotate-and-show-once where reveal is unavailable. Acceptance: U59,
  U60.
- **FR-VLT-22** (P0) Every vault change **must** write a hash-chained audit row in the same batch, with typed
  fields only, kept 400 days by age and never trimmed by count; every row except authentication failures
  **must** emit an event carrying the chain head. Acceptance: U61, U62, U63, U83.
- **FR-VLT-23** (P0) The kill switches and `revoke-all` **must** stop redemption at the next request.
  Acceptance: U64, U65.
- **FR-VLT-24** (P0) Item deletion **must** crypto-shred with a receipt, and identity and tenant erasure **must**
  remove vault rows and objects with receipt fields and probes. Acceptance: U66, U67, U68.
- **FR-VLT-25** (P0) Documentation, the site and SDK descriptions **must** use the honest trust statement and
  **must not** claim zero knowledge or end-to-end encryption. Acceptance: U69.
- **FR-VLT-26** (P0) Approval surfaces **must not** render any agent- or key-written text as a link or markup.
  Acceptance: U84.
- **FR-VLT-27** (P1) The MCP server **must** offer the six vault tools only by permission, none of which returns a
  value or approves; the CLI **must** read values only from standard input or files. Acceptance: U71, U72.
- **FR-VLT-28** (P0) A consumer holding a value across requests **must** learn of a use's early closure within 30
  seconds through the use status route, and drop the value and its connections. Acceptance: U82.
- **FR-VLT-29** (P1) Pylota Identity **must** offer the session import route that lets an integrator migrate an
  existing sealed session store with idempotent, auditable imports. Acceptance: U50.
- **FR-VLT-30** (P1) Every outbound fetch the platform makes, and every consumer fetch in the conformance
  fixtures, **must** refuse NAT64, 6to4 and cloud-metadata destinations by the embedded IPv4. Acceptance: U81.

## 4. Amendments to existing requirements

- **FR-KEY-1**: add at the end: "Identity keys **must not** hold `vault:use`, `vault:approve` or `vault:admin`
  ([Vault § 17.2](design/vault.md#172-what-each-kind-of-key-may-hold))."
- **FR-TEN-4**: add `vault.min_approval` and `vault.codes_into_fills_only` to the list of guard fields in
  parentheses, and "`vault.allowed`" to the platform-only fields.
- **FR-IDN-10**: add at the end: "A ledger entry **may** list up to five `login_domains` where the account signs
  in, shown apart on the approve page, and **may** carry a `vault_login` that the approval turns into a vault
  login item and its first grant ([Vault § 3.3](design/vault.md#33-ledger-binding),
  [§ 3.5](design/vault.md#35-a-login-born-from-a-ledger-approval))."
- **FR-IN-5**: add: "A verification message for a vault login item's channel **must** be stored `hidden` with
  `quarantine_reason = 'vault_code'`, withheld from every agent surface and from API keys whatever their
  permissions, and never released (FR-VLT-15)."
- **FR-IN-11**: add: "`wait` with `kind=verification` **must** answer `403 policy_denied` when the tenant's
  `vault.codes_into_fills_only` is `true`."
- **FR-CON-6**: add: "Approving a vault grant or use, and activating a vault consumer, **must** follow the same
  rule (`Decision::ApproveVault`, `Decision::ActivateConsumer`); console reveal of a vault value **must** need
  re-authentication within 10 minutes."
- **FR-PRV-3**: add: "Identity and tenant scopes **must** also crypto-shred vault items and count
  `vault_items_deleted`, `vault_versions_shredded`, `vault_keys_shredded`, `vault_uses_deleted` and
  `vault_audit_deleted`."

## 5. In "## 10. Risks", add these rows to the table after the last row

| A vault value reaches a model through a consumer's mistake (a screenshot during a fill, an echoing response, a page that copies the value) | Values only through registered consumers bound by a contract with conformance fixtures; no model step during a fill; post-fill exposure search; response redaction; `value_exposed` blocks the item until rotation |
| The operator or Cloudflare can decrypt vault values | Stated plainly in every document; partner custody for partners who hold their own unlock key; no zero-knowledge claim, enforced by a CI check |
| `hpke` 0.14.1 may not build or meet the CPU budget on `wasm32-unknown-unknown` | Spike S-VLT1 first; the fallback implements the same RFC 9180 suite from pinned primitives, with the same wire format |

## 6. In "## 13. Business model and pricing", add this paragraph after the phone numbers paragraph

- **The vault** is part of the paid plans, with no separate price and no metering in v1.0; a Free-plan workspace
  cannot use it. Its limits (items per identity and per workspace, sessions, grants, uses) are in
  [Limits › Vault](../reference/limits.md#vault).
