Where this goes: `docs/src/reference/api.md`. (1) Six rows in "### Permissions" after `accounts:approve`; (2) a new top-level section "## Vault" after "## Service accounts" (after its `---`), or after the phone sections if they are pasted first. Source: [Vault](../project/design/vault.md).

## 1. Rows for the "### Permissions" table

| `vault:read` | Vault item metadata (never values), grants, uses, the tenant's vault status (tenant level and above) ([Vault](#vault)). Every key level; an identity key only for its own identity |
| `vault:write` | Create, update, archive, restore and rotate vault items; values are write-only; import a session. Every key level; an identity key only for its own identity's items |
| `vault:request` | Ask for vault grants; give up its own identity's grants; P1 `sign` uses. Every key level |
| `vault:use` | Create, cancel and mint tickets for vault uses: the integrator's orchestrator. Platform, partner and tenant keys; **never identity keys** |
| `vault:approve` | Approve and deny vault grants and uses, and loosen an item's approval level; a decision reserved for people. Includes `vault:read`. Platform, partner and tenant keys; never identity keys |
| `vault:admin` | Delete items, manage consumers, rotate the tenant's vault key, change custody, `revoke-all`. Includes `vault:read`. Platform, partner and tenant keys; never identity keys |

## 2. New section

## Vault

Logins, API keys and browser sessions that agents use without ever seeing them
([Vault guide](../guides/vault.md), [design](../project/design/vault.md)). Off unless the deployment sets
`PM_VAULT = "on"` and the platform sets the tenant's `vault.allowed: true` (`422 vault_disabled`); on a Stripe
deployment a Free-plan workspace gets `403 policy_denied` (`plan_required`). No route returns a value, a
ticket other than at minting, a packet, an unlock key other than at enrolment, or a TOTP seed. Every change
writes a row to the tenant's hash-chained vault audit and emits a `vault.*` [event](events.md#vault).

### Items

#### `POST /v1/identities/{identity_id}/vault/items` — `vault:write` · `POST /v1/tenants/{tenant_id}/vault/items` — `vault:write`, tenant, partner or platform key

An identity-scoped item (usable only by that identity; `account_id` required) or a tenant-scoped item (usable by
identities it is granted to; each grantee needs its own approved ledger entry). `Idempotency-Key` optional; a
replay returns the item.

```json
{ "kind": "login", "title": "TfL road user charging", "account_id": "sac_01JA…",
  "username": "compliance.brightwell@pylotamail.com", "origins": ["https://tfl.gov.uk"],
  "password": "…", "totp": "otpauth://totp/TfL:compliance?secret=…&period=30",
  "approval": "per_run", "otp_channel": { "kind": "email", "address": "compliance.brightwell@pylotamail.com",
  "sender_domains": ["tfl.gov.uk"] }, "rotation": { "remind_after_days": 90 } }
```

- `kind`: `login` (`password` as a string or `{ "generate": { "length", "classes" } }`, optional `totp`,
  `recovery_codes`, `otp_channel`), `api_key` (`secret`, `inject: { "scheme": "bearer" | "basic" | "header",
  "header_name"?, "username"? }`, P1 `sign`, optional `rotate_after_use`) or `session` (`state` in Playwright's
  `storageState` shape, `session: { "ttl_seconds" }`, optional `import: { "source": "pylota_sealed_session",
  "source_ref" }`).
- `origins`: 1–10 exact `https` origins, canonicalised; each must belong to the ledger entry's service domain or
  `login_domains` (`422 invalid_request`, `ledger_entry_not_approved` or `origin_not_in_ledger`).
- `approval`: `grant`, `per_run` or `per_use` (default).
- Returns `201` with the [item](#vault-item-object). Refusals: `422 vault_kind_refused` (cards, bank details,
  passkeys, refused domains, confusable origins), `422 vault_item_limit_reached`, `422 vault_session_too_large`,
  `409 identity_paused`, `403 tenant_suspended`. A session's response adds `cookies_dropped`,
  `clearance_cookies_dropped` and `origins_dropped`.

#### `GET /v1/identities/{identity_id}/vault/items` · `GET /v1/tenants/{tenant_id}/vault/items` — `vault:read`

Newest first; filters `kind`, `state`, `health`, `account_id`. The identity list includes tenant-scoped items
with an active grant for the identity.

#### `GET /v1/vault/items/{item_id}` · `PATCH /v1/vault/items/{item_id}` — `vault:read` · `vault:write`

`PATCH` changes `title`, `origins` (a removed origin leaves grants with the rest; an added one widens no grant),
`approval` (looser only by a person or a key that may decide, otherwise `403 permission_denied`,
`person_required`), `state` (`archived` or `active`), `otp_channel` and `rotation`. Values are changed only by a
new version.

#### `POST /v1/vault/items/{item_id}/versions` — `vault:write`

Rotates: the kind's values, and `overlap_hours` (0–168, default 24) for logins and API keys. Fields not given are
carried over (in partner custody that needs `X-Pylota-Identity-Vault-Unlock`). Open uses close (`item_rotated`).
Returns `201` with the item.

#### `DELETE /v1/vault/items/{item_id}` — `vault:admin`, never an identity key

Crypto-shreds every version, deletes the item's grants and closes its uses. Returns `200` with the shred receipt
(`versions_shredded`, `r2_objects_deleted`, `uses_closed`, `grants_deleted`, `restorable_until`, `custody`).

#### Vault item object

```json
{ "id": "vit_01JAD4…", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…", "kind": "login",
  "title": "TfL road user charging", "account_id": "sac_01JA…", "username": "compliance.brightwell@pylotamail.com",
  "origins": ["https://tfl.gov.uk"], "approval": "per_run", "state": "active", "health": "ok",
  "health_reason": null, "current_version": 3,
  "has": { "password": true, "totp": true, "recovery_codes": false, "email_otp": true, "sms_otp": false },
  "otp_channel": { "kind": "email", "address": "compliance.brightwell@pylotamail.com", "sender_domains": ["tfl.gov.uk"] },
  "rotation": { "remind_after_days": 90, "last_rotated_at": "…" }, "session": null, "managed_by": null,
  "created_by": { "key_id": "key_01J9…", "user_id": null }, "first_used_at": "…", "last_used_at": "…",
  "created_at": "…", "updated_at": "…" }
```

`title` was written by a person or an agent: show it as untrusted text.

### Grants

#### `POST /v1/identities/{identity_id}/vault/grants` — `vault:request`

```json
{ "item_id": "vit_01JAD4…", "operations": ["fill"], "origins": ["https://tfl.gov.uk"],
  "purpose": "Check PCN status each morning.", "max_uses": null }
```

Creates a grant `pending_approval` for 7 days. With `"approve": true`, `expires_in` and (from an API key)
`on_behalf_of`, a caller that holds `vault:approve` and may decide creates it `active`. `409 vault_grant_exists`,
`422 vault_grant_limit_reached`, `403 vault_not_granted` (`ledger_not_approved`), `404 vault_item_not_found`.

#### `GET /v1/identities/{identity_id}/vault/grants` · `GET /v1/tenants/{tenant_id}/vault/grants` · `GET /v1/vault/grants/{grant_id}` — `vault:read`

The tenant list is the approval queue (oldest pending first). Filters `status`, `item_id`.

#### `POST /v1/vault/grants/{grant_id}/approve` — `vault:approve`

`{ "expires_in": 604800, "note": "…", "on_behalf_of": "usr_8812" }`. `Idempotency-Key` required. A decision
reserved for people: an API key may approve only under the rule of
[Vault § 5](../project/design/vault.md#5-grants) (`403 permission_denied`, `person_required`); the key that
requested it never approves (`self_approval`); `409 vault_not_pending`.

#### `POST /v1/vault/grants/{grant_id}/deny` — `vault:approve` · `POST /v1/vault/grants/{grant_id}/revoke` — `vault:approve`, or `vault:request` from the grantee's own identity key

`{ "note" }` or `{ "reason" }`. Always allowed: they only keep a credential away. Revoking closes the grant's open
uses (`grant_revoked`).

#### Vault grant object

```json
{ "id": "vgr_01JAD5…", "tenant_id": "ten_01J9…", "item_id": "vit_01JAD4…", "identity_id": "idn_01J9…",
  "operations": ["fill"], "origins": ["https://tfl.gov.uk"], "status": "active",
  "purpose": "Check PCN status each morning.", "max_uses": null, "use_count": 12,
  "requested_by_key_id": "key_01J9…", "decided_by_key_id": "key_01J8…", "decided_by_user_id": null,
  "on_behalf_of": "usr_8812", "note": null, "decided_at": "…", "expires_at": "…",
  "revoked_at": null, "revoke_reason": null, "created_at": "…", "updated_at": "…" }
```

### Uses and tickets

#### `POST /v1/identities/{identity_id}/vault/uses` — `vault:use`, never an identity key

The orchestrator asks to use an item now. `Idempotency-Key` required.

```json
{ "item_id": "vit_01JAD4…", "operation": "fill", "phases": ["credentials", "email_otp"],
  "consumer_id": "vcn_01JAC2…", "run_id": "run_8f2c41", "origin": "https://tfl.gov.uk",
  "trigger": "login_form", "purpose": "Check PCN status for WM12ABC", "capture_into": null }
```

Returns `201` with `use` and, when no person must decide, `ticket` and `ticket_expires_at` (redeem within 60
seconds). A replay returns `ticket: null` and `ticket_replayed: false`. `operation: "sign"` (P1) takes
`vault:request`, may come from an identity key, and answers with the signature instead of a ticket. Errors:
`403 vault_not_granted`, `403 vault_refused`, `404 vault_consumer_not_found`, `403 consumer_not_active`,
`429 rate_limited` (`RL_VAULT`, or `details.bucket = "vault_pending"`).

#### `GET /v1/identities/{identity_id}/vault/uses` · `GET /v1/tenants/{tenant_id}/vault/uses` · `GET /v1/vault/uses/{use_id}` — `vault:read`

Never shows a ticket, its hash or a sealed key. The tenant list defaults to `status=pending_approval`, oldest
first. `404 vault_use_not_found`.

#### `POST /v1/vault/uses/{use_id}/approve` — `vault:approve` · `…/deny` — `vault:approve` · `…/cancel` — `vault:use`

`approve` takes `{ "digest", "note"?, "on_behalf_of"? }` (`Idempotency-Key` required); a digest that is not the
use's is `400 invalid_request` (path `digest`). It does not return a ticket.

#### `POST /v1/vault/uses/{use_id}/ticket` — `vault:use`

Mints a ticket for an approved use whose phases are unredeemed, replacing any earlier one. In partner custody,
also `X-Pylota-Identity-Vault-Unlock`. Returns `{ "ticket", "ticket_expires_at", "phases", "use_id" }`. Not
idempotent: each call mints a new ticket.

#### `POST /v1/vault/uses/{use_id}/signature` — `vault:request` (P1)

Once, for an approved `sign` use; the use then closes.

#### Vault use object

```json
{ "id": "vus_01JAD6…", "tenant_id": "ten_01J9…", "identity_id": "idn_01J9…", "item_id": "vit_01JAD4…",
  "grant_id": "vgr_01JAD5…", "operation": "fill", "phases": ["credentials", "email_otp"],
  "phases_redeemed": ["credentials"], "consumer_id": "vcn_01JAC2…", "run_id": "run_8f2c41",
  "origin": "https://tfl.gov.uk", "trigger": "login_form", "purpose": "Check PCN status for WM12ABC",
  "status": "in_use", "approval_reason": "per_run", "approved_by": "run", "digest": "c41a…",
  "deadline_at": null, "redeem_by": "…", "closes_at": "…", "close_reason": null, "outcome": null,
  "created_by_key_id": "key_01J9…", "decided_by_key_id": null, "decided_by_user_id": null,
  "on_behalf_of": null, "created_at": "…", "updated_at": "…" }
```

### Consumers

#### `POST /v1/vault/consumers` — `vault:admin`

```json
{ "kind": "browser", "name": "Browser fleet", "level": "tenant", "tenant_id": "ten_01J9…",
  "hosting": "self_hosted", "provider": null, "contract": "vault-consumer/1",
  "signing_jwk": { "kty": "OKP", "crv": "Ed25519", "x": "…" },
  "encryption_jwk": { "kty": "OKP", "crv": "X25519", "x": "…" },
  "declarations": { "session_recording": "off", "session_logging": "off", "captcha_solving": "off",
                    "extensions": "none", "honest_identity": "web_bot_auth",
                    "observation_paused_during_fill": true, "post_fill_check": true, "modes": ["cdp"] } }
```

Created `pending_activation`. A JWK with a private member is `400 invalid_request` (`private_key_supplied`);
`422 vault_consumer_limit_reached`.

#### `POST /v1/vault/consumers/{consumer_id}/activate` · `…/suspend` · `…/resume` · `…/rotate-keys` · `DELETE /v1/vault/consumers/{consumer_id}` · `GET /v1/vault/consumers` · `GET /v1/vault/consumers/{consumer_id}` — `vault:admin`

Activation and resume are decisions reserved for people (a platform key, or a signed-in owner or admin for a
tenant-level self-hosted consumer); `activate` takes `{ "note" }`. `rotate-keys` takes the new JWKs and
`overlap_seconds` (0–86,400, default 3,600). Suspension and revocation close the consumer's open uses.

### The tenant's vault

#### `GET /v1/tenants/{tenant_id}/vault` — `vault:read`, tenant, partner or platform key

`{ "enabled", "custody", "kid", "items", "active_grants", "pending_grants", "pending_uses", "retiring_versions" }`.

#### `POST /v1/tenants/{tenant_id}/vault/rotate-key` — `vault:admin`

A new tenant key; versions are re-wrapped by the every-minute sweep. In partner custody, needs the unlock key.

#### `POST /v1/tenants/{tenant_id}/vault/custody` · `DELETE /v1/tenants/{tenant_id}/vault/custody` — `vault:admin`, the tenant's own partner key

Enrol `{ "slots": 1 | 2 }`: answers once with the unlock keys (`pvu_…`) and `operator_copy_restorable_until`; a
replay answers without them (`secret_replayed: false`). Release needs a valid unlock key in the header.

#### `POST /v1/tenants/{tenant_id}/vault/revoke-all` — `vault:admin`

`{ "shred_sessions": true, "archive_items": false }`. Revokes every grant and closes every use; optionally deletes
every session version and archives every item.

#### `GET /v1/tenants/{tenant_id}/vault/audit` — `audit:read`, tenant, partner or platform key

Rows in `seq` order with `prev_hash` and `row_hash`; filters `item_id`, `identity_id`, `consumer_id`, `action`,
`since_seq`. Verify the chain from any row you stored.

### Consumer routes

No API key. Every request carries an RFC 9421 HTTP message signature made with the consumer's Ed25519 key over
`@method`, `@authority`, `@path` and `content-digest` (not for `GET`), with `created`, `expires` (at most 60
seconds later), a `nonce`, `keyid`, `alg="ed25519"` and `tag="pylota-identity-vault-consumer"`. Any failure is
`401 consumer_unauthenticated` with the same body
([Vault § 8.4](../project/design/vault.md#84-authenticating-a-consumer)).

| Route | Does |
|---|---|
| `GET /v1/vault/consumer/self` | The consumer's own record |
| `POST /v1/vault/consumer/redeem` | `{ ticket, phase, wait_seconds, context }`: redeems one phase; answers the HPKE-sealed packet ([§ 8.6](../project/design/vault.md#86-redemption-checks)) |
| `POST /v1/vault/consumer/report` | `{ ticket, phase?, outcome, final? }`: reports an outcome; answers with `closed` |
| `POST /v1/vault/consumer/write-back` | `{ ticket, kind, … }`: a refreshed session, a rotated secret or a password confirmation; answers with `closed` |
| `GET /v1/vault/consumer/uses/{use_id}` | The use's `status`, `closed` and `close_reason`, for a use bound to this consumer; at most 6 a minute per use |

The header `X-Pylota-Identity-Vault-Unlock` is accepted only from the tenant's own partner key on
`POST …/vault/uses` (when it mints), `POST /v1/vault/uses/{use_id}/ticket`, `…/signature`, `…/vault/rotate-key`,
`POST …/versions` and `DELETE …/vault/custody`; anywhere else it is `400 invalid_request`
(`unlock_not_accepted`). It is never logged or stored.

---
