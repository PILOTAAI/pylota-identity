Where this goes: `docs/src/project/design/index.md`. (1) A row in "## Index" after the phone row (or after "Parity additions"); (2) rows in the "## Spikes" table after the last spike. Source: [Vault](vault.md).

## 1. Index row

| [Agent credential vault](vault.md) | Logins, API keys and browser sessions used without reveal ([ADR 0021](../adr/0021-agent-vault.md)): the honest trust model, items bound to approved ledger entries and exact origins, per-tenant X25519 keys with per-version data keys (HPKE) and optional partner custody, grants and uses approved by people and bound to a digest, one-use tickets, registered consumers with signed requests and sealed packets, the consumer contract `vault-consumer/1` and its conformance fixtures, TOTP and email and SMS codes into fills, sessions, rotation, signing, console reveal, managed secrets for other services, the hash-chained audit, kill switches, erasure, MCP and the CLI, and the Pylota cut-over (FR-VLT-1–30). |

## 2. Spikes

These three run at the start of M42 and M43, not in M1, against staging.

| **S-VLT1** HPKE on wasm32 | `hpke` 0.14.1 (`default-features = false`; `x25519`, `aes`, `alloc`) builds for `wasm32-unknown-unknown` with randomness from `platform::Rng`, passes the RFC 9180 vectors for the suite, and seals and opens 1 MiB within 50 ms of CPU in a Worker | All vectors pass; within budget | The same suite in `core::crypto::hpke` from the pinned `x25519-dalek`, `hkdf`, `sha2` and `aes-gcm`, checked against the same vectors ([ADR 0021](../adr/0021-agent-vault.md) decision 4) | [Vault § 4.3](vault.md#43-data-keys-and-field-ciphertext) |
| **S-VLT2** Audit sequence under contention | The guarded batch of [Vault § 14.2](vault.md#142-appending) under 50 concurrent writers to one tenant, on `wrangler dev` and staging D1 | No gap or duplicate `seq`; every change with its row; retries within three | Serialise vault writes per tenant through `TenantQuota` | [Vault § 14](vault.md#14-audit-and-events) |
| **S-VLT3** Consumer signatures | RFC 9421 Ed25519 verification with `core::httpsig` and `verify_strict`, against RFC 9421 Appendix B and signatures from the consumer SDK helpers | All verify; tampered components fail | None needed | [Vault § 8.4](vault.md#84-authenticating-a-consumer) |
