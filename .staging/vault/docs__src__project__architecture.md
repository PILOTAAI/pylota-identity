Where this goes: `docs/src/project/architecture.md`. (1) A paragraph at the end of "### Entry points"; (2) a row or sentence in "## 2. Storage"; (3) a new subsection "### 4.8 The vault" at the end of "## 4. Main flows"; (4) a line in "## 6. Code layout". Source: [Vault](design/vault.md).

## 1. Entry points

The vault adds five **consumer routes** under `/v1/vault/consumer/*` on the API host. They take no API key: each
request is signed by a registered consumer with RFC 9421 (Ed25519), and the only values that ever leave the
Worker do so here, HPKE-sealed to the consumer's own key ([Vault § 8](design/vault.md#8-consumers)).

## 2. Storage

Vault items, versions, grants, uses, consumers and the hash-chained vault audit live in D1; session versions are
ciphertext in R2 under `t/{tenant_id}/vault/`, never in the backup bucket. Every value is sealed under a per-version
data key, itself sealed to a per-tenant X25519 key whose private half is sealed under the master key (and under a
partner's unlock key in partner custody).

## 3. 4.8 The vault

```text
agent ──MCP/REST (identity key)──▶ vault_request_grant ──▶ person approves (console / partner app)
orchestrator ──REST (vault:use)──▶ POST …/vault/uses ──▶ [person approves] ──▶ ticket (once)
consumer ──signed POST /v1/vault/consumer/redeem──▶ Worker: checks binding and live state, opens the DEK
         with the ticket, decrypts one phase, seals it to the consumer's X25519 key ──▶ packet
consumer fills at the exact origin, reports, writes back; every step: D1 batch + audit row + event
```

The vault makes no outbound request with a secret ([Vault § 1.4](design/vault.md#14-credentialed-fetch-is-deferred)).
One-time codes come from the identity's own mailbox (rule 4b) or number, never through the model.

## 4. Code layout

`crates/core/src/vault/` and `crates/core/src/crypto/{hpke.rs, pv1.rs}` (pure), `crates/worker/src/vault/`,
`handlers/vault.rs`, `handlers/vault_consumer.rs`, `auth/consumer.rs`, `crates/conformance/vault/`.
