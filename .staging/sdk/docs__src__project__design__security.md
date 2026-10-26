Where this goes: `docs/src/project/design/security.md`. Two changes in "## 11. Supply chain" ([ADR 0022](../adr/0022-sdk-languages.md)).

## 1. In the row "Signed releases", after "the TypeScript SDK fixtures tarball", insert

(which also carries the SDK conformance suite), and the `ts-`, `py-` and `go-tests-required.json` files

## 2. After the row "npm package", add these rows

| PyPI package | `pylota-identity` depends at run time only on `httpx`, with `websockets` and `cryptography` behind extras, each bounded below the next minor; it has no build hooks beyond its build backend. It is built in its own repository, `PILOTAAI/pylota-identity-python`, only from a contract and fixtures whose release signature verifies; its sdist and wheel are attested, listed in a `SHA256SUMS` the owner signs offline, and uploaded only by that repository's `release-publish.yml` through PyPI trusted publishing (environment `release`), with Sigstore attestations; the project allows no API token. Pre-releases are never published ([SDKs, skills and agent tooling § 3.5](sdk-and-skills.md#35-python)) |
| Go module | `github.com/PILOTAAI/pylota-identity-go` requires only `github.com/coder/websocket`; generators and linters live in a separate tools module. A version is published by its tag, so the tag is pushed only by `release-publish.yml`, after the owner's signature over the module zip's SHA-256 and `h1:` hash verifies; the workflow then requires the module proxy's and the checksum database's hash to equal the signed one, and a mismatch is retracted. No pre-release tag is pushed ([§ 3.6](sdk-and-skills.md#36-go)) |
| Vault consumer code | The consumer helpers of every SDK (origin canonicalisation, request signing, packet opening, redaction) are a separate entry point, never loaded by the default import, generated against vectors produced by `crates/core` ([§ 3.4](sdk-and-skills.md#34-the-vault-consumer-entry-point)) |
