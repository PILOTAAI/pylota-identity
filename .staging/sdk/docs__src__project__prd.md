Where this goes: `docs/src/project/prd.md`. Four changes, each introduced by where it goes. Text is copied from
`docs/src/project/design/sdk-and-skills.md` (ADR 0022) with links rewritten for the PRD's folder; a difference
between the two is a defect.

## 1. In "## 5. Scope and priorities", row "Integrations"

In the P1 column, after "TypeScript SDK `@pylotahq/identity` (FR-SDK-2–9);", add:

Python SDK `pylota-identity` and Go SDK `github.com/PILOTAAI/pylota-identity-go`, each in its own repository, and
`@pylotahq/identity` as the JavaScript SDK (FR-SDK-25–27); the shared SDK conformance suite (FR-SDK-28); phone and
vault helpers and the vault consumer entry point in every SDK (FR-SDK-29–31); SDK release and example checks
(FR-SDK-32–33);

In the "Later" column, delete "Python SDK;" (the cell then reads "MCP OAuth 2.1; TypeScript agent-assertion
verifier").

## 2. In "### Non-goals (v1.0)", delete this bullet

- A Python SDK (P2). v1.0 offers Python users the OpenAPI document, the MCP server and the CLI
  ([SDKs, skills and agent tooling §3](design/sdk-and-skills.md#3-python)).

## 3. In "### 6.11 Interfaces", replace FR-SDK-3, FR-SDK-7 and FR-SDK-15 with

- **FR-SDK-3** The SDKs **must never** generate an idempotency key for an operation marked
  `x-idempotency: required` (the mail sends, calendar reply and draft send, and the phone and vault
  operations their designs mark, such as a phone purchase, an SMS send and a vault use), and
  **must** send the same key and the same body bytes on every retry of one call. Every SDK
  **must** retry only failures that the error envelope marks `retryable`, and network failures and proxy
  `502`, `503` and `504` only for a `GET` or a request that carries an idempotency key; **must** honour
  `Retry-After` up to a bound and throw beyond it; and **must** end a call whose outcome it could not learn
  with an error that carries the key.
- **FR-SDK-7** The SDKs **must** tolerate unknown fields, enum values, event types and error codes from a
  newer deployment. Every SDK **must** report an operation that a deployment lacks as unsupported
  by that deployment, never as a missing resource.
- **FR-SDK-15** The docs **must** include guides for the Vercel AI SDK, the OpenAI Agents SDK, the Claude
  Agent SDK and LangChain, TypeScript, JavaScript, Python and Go SDK guides, a vault consumer guide and an
  agent skills guide, whose code examples type-check (or compile) in CI against pinned versions.

## 4. In "### 6.11 Interfaces", after FR-SDK-15 (before FR-API-3), add

(Not to paste: FR-SDK-16 – FR-SDK-24 stay unused, left free by the allocation of 2026-10-25; do not renumber.
All are P1, as section 1's row says.)

- **FR-SDK-25** The project **must** ship a Python SDK, the PyPI package `pylota-identity` (import
  `pylota_identity`), built in its own repository `PILOTAAI/pylota-identity-python`, with blocking and
  `asyncio` clients that have exactly one method per `openapi.yaml` operation not marked `x-sdk: false`. It
  **must** support every CPython line from its release to its end of life, starting with 3.11, and **must**
  depend at run time only on `httpx`, with `websockets` and `cryptography` behind the extras `events` and
  `vault-consumer` ([SDKs, skills and agent tooling § 3.5](design/sdk-and-skills.md#35-python)).
- **FR-SDK-26** The project **must** ship a Go SDK, the module `github.com/PILOTAAI/pylota-identity-go`,
  built in its own repository of that name, with exactly one method per such operation. It **must** support
  the Go releases the Go project supports, starting with 1.26, and **must** depend only on the standard
  library and `github.com/coder/websocket`; tools **must** live in a separate module
  ([§ 3.6](design/sdk-and-skills.md#36-go)).
- **FR-SDK-27** `@pylotahq/identity` **must** be usable from plain JavaScript with the same run-time
  refusals as from TypeScript; there **must not** be a second JavaScript package
  ([§ 3.7](design/sdk-and-skills.md#37-javascript)).
- **FR-SDK-28** Every SDK **must** implement FR-SDK-3 – FR-SDK-8 with the behaviour the shared conformance
  suite of each release defines, and **must not** adopt a contract whose conformance cases it does not all
  pass. The suite **must** be generated from `crates/core`, and published in the release's
  signed fixtures asset ([§ 3.2](design/sdk-and-skills.md#32-the-shared-conformance-suite)).
- **FR-SDK-29** Every SDK **must** offer the one-call phone purchase, phone set-up status, `wait` for SMS and
  SMS outcome classification, returning a pending approval and a taken number as typed results, never
  retrying a purchase with another number and never resending an `uncertain` SMS. Its approval methods for
  phone requests, vault grants and vault uses **must** take the digest from the caller and **must never** fetch
  one to fill in or retry an approval ([§ 3.3](design/sdk-and-skills.md#33-phone-and-vault-coverage)).
- **FR-SDK-30** Every SDK **must** offer the vault consumer helpers (origin canonicaliser, request signer,
  redeem with HPKE packet opening, report, write-back, the ticket-origin check and the redactor) in an entry
  point separate from its default import, producing exactly the outputs of the conformance vectors; it
  **must never** retry a redemption after the request may have reached the service, **must** give packet
  fields and keys no string, JSON or error form, and **must** let the caller overwrite packet plaintext
  ([§ 3.4](design/sdk-and-skills.md#34-the-vault-consumer-entry-point)).
- **FR-SDK-31** A vault ticket **must** be treated as a key in every SDK: never in a URL, a log, an error or a
  string form beyond its lookup prefix.
- **FR-SDK-32** Each Python and Go SDK release **must** name the Pylota Identity release whose contract it
  was generated from, **must** have its artefacts listed in an owner-signed `SHA256SUMS`, and **must** be
  published only by its repository's release workflow: Python by PyPI trusted publishing with attestations,
  Go by pushing the tag after signing and checking the proxy's `h1:` hash against the signed one. A
  pre-release **must not** be published ([§ 3.5](design/sdk-and-skills.md#35-python), [§ 3.6](design/sdk-and-skills.md#36-go)).
- **FR-SDK-33** Every code example of an SDK, on the landing page and in the docs, **must** be checked in the
  CI of the SDK it uses at the contract's tag, and each SDK's package metadata **must** state the licence
  `Apache-2.0`, with `LICENSE` and `NOTICE` in every published artefact.


---

Under `## 12. Licensing`, after the bullet that begins "Contributions are licensed to TREFT LTD":

- The client libraries are licensed differently from the service: the TypeScript, Python, Go and Rust SDKs, the
  crate `pylota-identity-api-types` they depend on, and the agent skills are under the **Apache License 2.0**
  ([ADR 0022](adr/0022-sdk-languages.md) decision 9). Everything else in this repository stays FSL-1.1-ALv2.
