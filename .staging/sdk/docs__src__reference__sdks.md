Where this goes: a new page, `docs/src/reference/sdks.md`, with the text below (everything after the line `---`). It is added to `SUMMARY.md` by `.staging/sdk/docs__src__SUMMARY.md`. Until M36, M47 and M48 publish the packages, the page says so in its first table's "Status" column; each milestone changes its row to "Published" in its own pull request.

---

# SDKs

Pylota Identity has four SDKs. All of them follow one specification
([SDKs, skills and agent tooling](../project/design/sdk-and-skills.md)) and pass the same conformance suite,
published with every release, so a retry, an `uncertain` send, a webhook or an event stream behaves the same in
each.

| Language | Package | Install | Source | Status |
|---|---|---|---|---|
| Rust | crate `pylota-identity` | `cargo add pylota-identity` | [`crates/sdk`](https://github.com/PILOTAAI/pylota-identity/tree/main/crates/sdk) | With each product release (M16) |
| TypeScript and JavaScript | `@pylotahq/identity` | `npm install @pylotahq/identity` | [`PILOTAAI/pylota-identity-typescript`](https://github.com/PILOTAAI/pylota-identity-typescript) | Not yet published (M36) |
| Python | `pylota-identity` | `pip install pylota-identity` (add `[events]` for the live stream, `[vault-consumer]` for vault consumers) | [`PILOTAAI/pylota-identity-python`](https://github.com/PILOTAAI/pylota-identity-python) | Not yet published (M47) |
| Go | `github.com/PILOTAAI/pylota-identity-go` | `go get github.com/PILOTAAI/pylota-identity-go` | [`PILOTAAI/pylota-identity-go`](https://github.com/PILOTAAI/pylota-identity-go) | Not yet published (M48) |

## Supported runtimes

| SDK | Supported |
|---|---|
| TypeScript and JavaScript | Node.js 22, 24 and 26; Cloudflare Workers; Deno 2; Bun 1. ES modules only; from CommonJS use `await import()` |
| Python | CPython 3.11, 3.12, 3.13, 3.14 and 3.15 |
| Go | Go 1.26 and 1.27 |

A runtime line is supported from its first release to its end of life, and leaves this table in the SDK's next
minor release after that.

## Versions and the contract

An SDK's major version is the API's path version: every `1.x` SDK works with every deployment that serves `/v1`,
for the operations both know. Each SDK release names the Pylota Identity release whose `openapi.json` it was
built from:

| SDK | Where the contract version is |
|---|---|
| TypeScript | `package.json` `pylotaIdentity.contract`; `@pylotahq/identity/openapi.json` is the contract itself |
| Python | `pylota_identity.CONTRACT` |
| Go | `identity.Contract` |

An SDK newer than a self-hosted deployment reports an operation the deployment lacks as "unsupported by the
server", never as a missing resource.

## The rules every SDK keeps

- A send, reply, reply-all, forward, calendar reply, draft send, phone purchase, SMS send, placed call or vault
  use needs your idempotency key. The SDK never makes one up, and every retry sends the same key and the same
  bytes ([Sending and safe retries](../guides/sending.md)).
- Only errors marked `retryable` are retried. A call whose outcome the SDK could not learn ends in an
  "outcome unknown" error that carries the key: retry it later with that key, never a new one.
- `uncertain` messages and SMS are never resent by the SDK.
- Webhooks are verified over the bytes you received, not a parsed body.
- An approval (of a phone request, a vault grant or a vault use) is sent with the digest you were shown; the SDK
  never fetches a newer one.
- The API key, stream tokens and vault tickets never appear in errors, logs or string forms. Redirects are not
  followed. The SDKs refuse to run in a browser.

## Vault consumers

Code that redeems vault tickets (a browser fleet, an egress proxy, an executor) uses a separate import, never
the default one:

| SDK | Import |
|---|---|
| Rust | feature `vault-consumer`, `pylota_identity::vault_consumer` |
| TypeScript and JavaScript | `@pylotahq/identity/vault-consumer` |
| Python | `pylota_identity.vault_consumer` (install `pylota-identity[vault-consumer]`) |
| Go | `github.com/PILOTAAI/pylota-identity-go/vaultconsumer` |

It canonicalises origins, signs consumer requests, opens packets and redacts values from responses exactly as
the service expects. A consumer must still meet the consumer contract and pass the conformance fixtures
(`idvy vault conformance --consumer DIR`) before it is activated
([Vault › The consumer contract](../project/design/vault.md#85-the-consumer-contract)).

## Verifying what you install

| SDK | How |
|---|---|
| Rust | The crate is published from the signed product release ([Security](../guides/security.md)) |
| TypeScript | `npm audit signatures` checks the registry signature and the provenance attestation |
| Python | PyPI shows the attestation of each file; the release's `SHA256SUMS` is signed by the owner (trusted comment `pylota-identity-python v{version}`); install with `pip install --require-hashes` from a lock file |
| Go | `go mod verify` and the Go checksum database; the release's signed `SHA256SUMS` lists the module's `h1:` hash (trusted comment `pylota-identity-go v{version}`) |

## Licence

Every SDK, and every agent skill, is licensed under the Apache License 2.0, so it can be installed and shipped
inside your own application without a licence review beyond the usual one. The Pylota Identity service itself is
source available under FSL-1.1-ALv2. The Go package documentation is on this page as well as on pkg.go.dev, so
the reference for every language is in one place.

## Go

The Go package reference is generated from the module's doc comments by the Go SDK's release (M48) and replaces
this paragraph. Until then, `go doc github.com/PILOTAAI/pylota-identity-go` shows it locally once the module is
published.
