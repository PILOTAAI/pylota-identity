# 0021 An agent credential vault inside Pylota Identity, used without reveal

| | |
|---|---|
| Status | Proposed |
| Date | 2026-10-26 |
| Deciders | The owner (TREFT LTD) |
| Related | [Agent credential vault](../design/vault.md), FR-VLT-1 to FR-VLT-30, edge rows U1–U85, milestones M42–M45, [ADR 0012](0012-service-sign-up-ledger.md), [ADR 0015](0015-solo-operator.md), [ADR 0019](0019-pylota-identity.md), [ADR 0020](0020-phone-numbers.md) |

## Context

The owner decided on 2026-10-24 that Pylota Identity gives agents a vault for the credentials they need at
third-party services, beside their identities and the service sign-up ledger ([ADR 0019](0019-pylota-identity.md)).
Its first customer, Pylota, keeps browser sessions for its agents in its own sealed store today (reviewed
privately on 2026-10-24) and will move them into the vault.

The facts the decision rests on:

- **Where code runs.** Workers run customer code in V8 isolates, many to a process; Cloudflare describes isolation
  and Spectre mitigations, not hardware attestation. To a Worker a secret is "no different" from an environment
  variable. Searches of Cloudflare's documentation found no confidential-computing feature for Workers (Workers
  security model and Secrets pages, read 2026-10-24). A vault that must sign in, compute codes and seal packets on
  its own therefore decrypts inside the Worker, and its operator and Cloudflare are inside the trust boundary.
- **What others do.** 1Password decrypts for its agent features inside an attested AWS Nitro enclave, which has no
  counterpart on Workers (1Password study, read 2026-10-24). Infisical's open-source Agent Vault is a forward
  proxy that attaches stored credentials to agents' outbound requests; at `d5836ec` (v0.40.2, read 2026-10-26) it
  streams upstream responses back unmodified, substitutes a declared placeholder wherever it appears in a
  declared request surface, injects into plain `http://` requests to a configured host, checks the proxy session
  only when a tunnel opens, prevents self-approval only for agents given its `proxy` role, and keeps its request
  log as at most 10,000 rows per vault with the oldest deleted first.
- **Passkeys.** The Chrome DevTools Protocol's WebAuthn domain takes a credential's raw private key and simulates
  user presence, defeating the properties that make a passkey resist phishing; the FIDO Alliance's agentic
  authentication working group (formed 2026-04-28) had published nothing (read 2026-10-24).
- **Hosted browsers.** Browserbase records and logs sessions and solves CAPTCHAs unless each is turned off when the
  session is created (read 2026-10-24).
- **HPKE in Rust.** `hpke` 0.14.1 implements RFC 9180 with `x25519-dalek` 3.0.0, `aes-gcm` 0.11 and `hkdf` 0.13,
  MIT or Apache-2.0, `no_std` with `alloc` (crates.io, read 2026-10-24). Whether it builds and runs within budget
  on `wasm32-unknown-unknown` is not yet shown.

## Decision

1. **Placement.** The vault **must** live inside Pylota Identity, beside identities and the service sign-up
   ledger, and every usable item **must** be bound to an approved ledger entry for its site.
2. **Use without reveal.** Agents **must never** receive a value. A value leaves the vault only sealed to a
   registered consumer's key (a browser, an egress proxy or an executor), for one phase of a one-use ticket bound
   to the consumer, the run, the exact origin and a short window, which the integrator's orchestrator (never a
   model) creates from a deterministic trigger. Identity keys **must never** hold `vault:use`, `vault:approve` or
   `vault:admin`.
3. **People approve.** A person (or a platform or partner key under `vault.key_approval`, with `on_behalf_of`)
   **must** approve every grant, and every use whose level requires it, bound to the use's digest. The key that
   asked **must never** approve. No surface renders agent-written text as a link.
4. **Keys.** One X25519 key per tenant, sealed under the master key; one random data key per item version, sealed
   to the tenant key with HPKE base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, AES-256-GCM; fields under
   AES-256-GCM with AAD binding tenant, item, version and purpose. No stored secret is derived from another; the
   one derivation is HPKE's own per-seal key schedule. **Fallback:** if spike S-VLT1 shows `hpke` 0.14.1 does not
   build, or does not meet the CPU budget, on `wasm32-unknown-unknown`, the same suite is implemented in
   `core::crypto::hpke` from the already-pinned `x25519-dalek`, `hkdf`, `sha2` and `aes-gcm` and checked against
   the RFC 9180 vectors. The wire format is RFC 9180's either way, so nothing stored changes.
5. **The optional partner-held unlock key.** A tenant's own partner **may** move the tenant into partner custody:
   the tenant's private key is then wrapped under one or two unlock keys the Worker generates and returns once, and
   deleted from operator custody in the same batch. Unwrapping routes **must** then require the unlock key in a
   request header from the tenant's partner key; sealing **must not**. In this mode a copy of the database and the
   master key is not enough to decrypt, the console cannot reveal, and lost unlock keys **must** mean lost values,
   with no recovery path. It is opt-in, P1, built in M42 and enabled per tenant. It is decided apart from the rest
   because it trades recoverability for a narrower trust boundary, and only a partner that runs its own key
   custody should take that trade.
6. **Honesty.** The documentation, site and SDK descriptions **must** use the trust statement of the design word for
   word and **must not** claim zero knowledge or end-to-end encryption; a check in CI refuses any such claim that is not a denial.
7. **Refusals.** The vault **must** refuse payment cards and bank details (by kind and by a Luhn check of every
   value), passkeys and any WebAuthn emulation, CAPTCHA solving and the storage of bot-challenge clearance cookies,
   plaintext export to any agent, model, MCP client or key, and self-approval.
8. **Credentialed fetch is deferred.** The vault **must not** make outbound requests with a secret on an agent's
   behalf in v1. The response of such a request is the leak: a public credential proxy shows both failure modes
   (unredacted responses, placeholders substituted into fields a site publishes). Pylota's API calls already run
   in its own servers through an `executor` consumer, so nothing in v1 needs it. It is revisited with an ADR when a
   customer without an executor asks.
9. **Codes into fills.** TOTP codes **must** be computed inside the vault; verification codes on a vault login's
   email or SMS channel **must** be withheld from every agent surface and delivered only into an open phase, once.
10. **The consumer contract** (`vault-consumer/1`, C1–C21) **must** be published, versioned and enforced where
    the vault can observe it, with conformance fixtures every consumer passes before activation, including the
    rules that the public proxy lacks: TLS only, a placeholder substituted only in the recorded field of one
    request to the recorded path, no WebSocket substitution, no redirects followed with a value, redaction
    robust to compression, chunking and headers, and closure learned within 30 seconds.
11. **Audit.** Every change **must** write a hash-chained audit row in its own batch, kept 400 days by age and never
    trimmed by count, with the chain head in every event.
12. **Replacement.** The vault replaces Pylota's sealed session store through an idempotent import route.

## Risk tier, flag, telemetry and rollback

**T2.** The vault holds third-party credentials, TOTP seeds and session cookies, decrypts them in a shared runtime,
and hands them to integrator components across a network. The build needs a security review, an independent
adversarial check aimed at reveal paths, approval binding, ticket replay and consumer impersonation, and the
owner's sign-off before each milestone merges.

- **Flag:** `PM_VAULT` (`off` by default) and the platform-only policy field `vault.allowed`.
- **Kill switches:** `PM_VAULT = "off"`, the ops switch `vault_redeem`, `vault.allowed: false`, `revoke-all`,
  consumer suspension.
- **Telemetry:** `vault_value_exposed_total`, `vault_audit_append_failures_total`,
  `vault_placeholder_misplaced_total`, `vault_unlock_failures_total`, `vault_consumer_reports_total{outcome}`,
  redemption latency; each rollout stage waits seven days with no audit append failure, no exposure outside the
  fixtures and no unexplained placeholder report.
- **Rollback:** additive D1 tables and nullable columns. Turning the flag off stops every use and redemption and
  keeps every item; it never releases a withheld code.

## Consequences

- Agents can sign in, use one-time codes and call APIs with credentials they never see; operators approve in one
  place, with a verifiable record.
- TREFT and Cloudflare stay able to decrypt in operator custody, and the documents say so.
- Integrators must build or adopt consumers and pass the conformance fixtures; the fixtures and a reference
  consumer ship in this repository.
- The build grows by four milestones (M42–M45), six permissions, two `Decision` arms, ten D1 tables, mailbox
  columns and a consumer authentication scheme.
- Follow-up: counsel's answers ([Vault § 23](../design/vault.md#23-questions-for-counsel)); credentialed fetch if a
  customer needs it; the FIDO Alliance's agent work for passkeys.

## Alternatives considered

- **A separate vault product, or a third-party one.** 1Password's agent features and Infisical's Agent Vault were
  studied. Neither binds use to an approved service account, per-use approval and an identity's own mailbox for
  codes; Infisical's runs as a second server with its own store and trust boundary and would need its credential
  store replaced to honour per-use approval. Refused; their good patterns are adopted.
- **Credentialed fetch through the vault.** Simplest for integrators, but it returns responses that can carry the
  secret and makes the Worker a general proxy. Deferred (decision 8).
- **Zero-knowledge, client-side encryption.** The vault could not then fill, compute codes or seal packets on its
  own; every use would need a person's device online. Refused for v1, and never claimed.
- **Giving agents values with a short time to live.** The model would then hold a password it could repeat to
  anyone who asks. Refused.
- **Passkey support through CDP emulation.** Defeats the phishing resistance that is the reason to use passkeys.
  Refused until a standard for agents exists.
