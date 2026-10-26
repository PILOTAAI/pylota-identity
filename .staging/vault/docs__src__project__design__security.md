Where this goes: `docs/src/project/design/security.md`. Insertions in seven sections, each introduced by where it goes. Source: [Vault](vault.md) ([ADR 0021](../adr/0021-agent-vault.md)).

## 1. In "## 1. Assets and invariants", add these rows to the assets table

| Vault values: passwords, TOTP seeds, recovery codes, API secrets, session cookies | Never returned to an agent, a model or an API key; decrypted only inside the Worker for one request; reach only a registered consumer, sealed to its key, for one phase of one ticket ([Vault](vault.md)) | `it::vault::items_write_only`, `it::vault::redemption_binding`, `xtask::vault_types_have_no_debug` |
| The tenant vault keys and partner unlock keys | Private halves sealed under the master key (operator custody) or also under the partner's unlock key (partner custody); unlock keys never stored | `it::vault::custody`, `core::crypto::hpke::rfc9180_vectors` |

## 2. In "## 2. Trust boundaries", add to the diagram's legend

```text
 TB9: integrator consumers (browser fleet, egress proxy, executor) ── signed HTTPS ──▶ /v1/vault/consumer/*;
      values leave the Worker only here, HPKE-sealed to the consumer's X25519 key
```

## 3. In "## 3. STRIDE threat model", a new subsection after "### 3.8 TB8"

### 3.9 TB9: vault consumers → the vault

| | Threat | Control | Test |
|---|---|---|---|
| S | Someone impersonates a consumer to redeem tickets | RFC 9421 Ed25519 signatures with a 30-second clock window, 60-second expiry and stored nonces; keys generated where used (a JWK with `d` refused); activation by a person or the platform | `it::vault::consumer_signatures` |
| S | A model or page obtains a ticket and redeems it | Tickets go only to the orchestrator; each is bound to one consumer, so another caller cannot redeem it; 60 seconds to first redemption; one redemption per phase | `it::vault::use_lifecycle`, `it::vault::consumer_reach` |
| T | A consumer fills on the wrong page, frame or form | The context of § 8.6 step 6 checked at redemption; C4, C5 and C11 in the consumer contract; conformance fixtures | `it::vault::redemption_binding`, `conf::vault::*` |
| R | A consumer or orchestrator denies a use | Hash-chained audit rows in the batch of each change, with the chain head in every event | `it::vault::audit_chain_and_age_only` |
| I | A value leaks through a response, a log, a screenshot, a placeholder in a writable field, a redirect or a cleartext request | No route returns a value; value types have no `Debug`; C10–C14, C17 and C21 (redaction robust to compression and chunking, placeholders pinned to the recorded field, TLS only, no redirects, closure within 30 s) | `xtask::vault_types_have_no_debug`, `conf::vault::echo_gzip`, `conf::vault::proxy_second_form`, `conf::vault::proxy_redirect_307` |
| D | A consumer floods redemption or status | Per-network failed-authentication bucket; `RL_VAULT`, `RL_VAULT_STATUS`; 100 pending uses per tenant | `it::vault::consumer_signatures` |
| E | An agent approves its own grant or starts a use | Identity keys cannot hold `vault:use`, `vault:approve` or `vault:admin`; the requesting key never approves; approval is a decision reserved for people | `it::vault::identity_key_levels`, `it::vault::grant_decisions` |

## 4. In "### 4.6 Creating keys (FR-KEY-1)", the permissions-by-level table

Add `vault:use`, `vault:approve` and `vault:admin` to the row of tenant-only permissions ("never on identity
keys"), and `vault:read`, `vault:write`, `vault:request` to "Every other permission".

## 5. In "### 5.1 Deny-by-default router table"

Add after the paragraph on empty permission lists: "The five vault consumer routes
(`/v1/vault/consumer/*`) are registered with `Auth::ConsumerSignature`, an empty permission list and
`Scope::VaultConsumer`: they take no API key, and an `Authorization` header on them is
`401 consumer_unauthenticated`. A consumer signature on any other route is ignored."

And in "### 4.7 Unauthenticated routes": "`/v1/vault/consumer/*`: no API key, but every request is authenticated
by the consumer's RFC 9421 signature ([Vault § 8.4](vault.md#84-authenticating-a-consumer))."

## 6. In "### 6.1 Inventory", add these rows

| Tenant vault keys (`vault_keys.private_sealed`) | Unwrapping one tenant's vault data keys | The Worker: 32 bytes from `platform::Rng` as an X25519 secret, sealed under `PM_MASTER_KEY` | With a D1 copy, every value of that tenant in operator custody |
| Partner unlock keys (`pvu_…`) | Unwrapping a tenant key in partner custody | The Worker at enrolment, returned once, never stored | With the master key and a D1 copy, that tenant's values |
| Vault data keys (`vault_item_versions.dek_sealed`) | Encrypting one item version's fields | The Worker, 32 bytes per version, HPKE-sealed to the tenant key | One version's values, with the tenant key |
| Vault tickets (`vtk_…`) | One use's redemption | The Worker at minting, returned once to the orchestrator; only SHA-256 stored | One use's phases, by the consumer it is bound to, within its window |
| Consumer keys (Ed25519, X25519) | A consumer's signatures and the packets sealed to it | The integrator, on the consumer's host (`idvy vault consumer-keys generate`) | Redeeming tickets bound to that consumer that the attacker also holds; reading packets captured in transit |

Add to the rules: "**Vault values** are never returned by any API, logged or exported; a person reveals one
only in the console after re-authentication ([Vault § 13.1](vault.md#131-human-reveal-p1))."

## 7. In "### 6.2 Rotation procedures"

Add to the sealed-column registry (`crates/core/src/sealed.rs`): `vault_keys.private_sealed` and
`vault_custody_slots.private_wrapped` (whose inner layer, under the unlock key, `idvy secrets rotate-master`
carries unopened). Add a procedure "**Tenant vault key**: `POST /v1/tenants/{tenant_id}/vault/rotate-key`; the
every-minute sweep re-wraps 200 versions a run ([Vault § 4.2](vault.md#42-the-tenant-vault-key))."

## 8. In "### 7.1 Choices", add these rows

| Vault data-key seal and consumer packets | HPKE base mode (RFC 9180): DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, AES-256-GCM | `hpke =0.14.1` (`default-features = false`, features `x25519`, `aes`, `alloc`), or the fallback of [ADR 0021](../adr/0021-agent-vault.md) decision 4 after spike S-VLT1 |
| Vault field ciphertext and the ticket copy of a data key | AES-256-GCM, random 96-bit nonce, AAD binding tenant, item, version and purpose (or use) | `aes-gcm` (the workspace's 0.11 pin) |
| Vault TOTP codes (RFC 6238) | HMAC-SHA1, -SHA256, -SHA512 per item | `hmac`, `sha1`, `sha2` |
| Vault consumer request signatures | RFC 9421 with Ed25519, `verify_strict` | `ed25519-dalek =3.0.0` |
| Vault signing (P1) | HMAC-SHA256; AWS SigV4 | `hmac`, `sha2` |
| Vault audit chain | SHA-256 over canonical JSON | `sha2 =0.11.0` |

## 9. In "## 9. SSRF controls", no change: the vault makes no outbound request in v1 ([Vault § 1.4](vault.md#14-credentialed-fetch-is-deferred)); the conformance fixtures check consumers against the embedded-IPv4 rules of § 9.1 (`it::security::ssrf_embedded_ipv4`).
