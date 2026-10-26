Where this goes: `docs/src/project/design/rust-workspace.md`. Four changes ([ADR 0022](../adr/0022-sdk-languages.md), [SDKs, skills and agent tooling § 3.8](sdk-and-skills.md#38-the-rust-sdk)).

## 1. In "## 9. `xtask`", row `cargo xtask release sign <tag>`, replace the sentence beginning "`--repo PILOTAAI/pylota-identity-typescript` does the same for a TypeScript SDK release" through "any other `--repo` is refused" with

`--repo` does the same for an SDK release of one of three repositories: `PILOTAAI/pylota-identity-typescript`
(trusted comment `pylota-identity-typescript v{version}`), `PILOTAAI/pylota-identity-python`
(`pylota-identity-python v{version}`) and `PILOTAAI/pylota-identity-go` (`pylota-identity-go v{version}`). The
draft release and the provenance are that repository's (`release.yml` on that tag, or for Go on the release
commit named in the draft), and it starts that repository's `release-publish.yml`; any other `--repo` is refused
([SDKs, skills and agent tooling § 2.16, § 3.5, § 3.6](sdk-and-skills.md#39-versions-support-and-verification))

## 2. In "## 9. `xtask`", add a row after `cargo xtask gen-skills` (or at the end of the table)

| `cargo xtask gen-sdk-vectors [--check]` | Writes the vault consumer vectors of `sdks/fixtures/conformance/vault-consumer/` (origins, signature bases and signatures, HPKE packets, redaction) from `crates/core` with fixed keys, nonces and ephemeral secrets; `--check` fails when a file would change ([SDKs, skills and agent tooling § 3.2](sdk-and-skills.md#32-the-shared-conformance-suite)) |

And in `cargo xtask release`'s row, after "`ts-tests-required.json`", add ", `py-tests-required.json` and
`go-tests-required.json`".

## 3. In "## 11. The Rust SDK (FR-SDK-1)", after the bullet "**Webhooks.**", add

- **Phone and vault.** `pylota_identity::phone` has the helpers of
  [SDKs, skills and agent tooling § 3.3](sdk-and-skills.md#33-phone-and-vault-coverage) (`buy_phone_number`,
  `phone_setup`, `wait_for_sms`, `sms_outcome`), and the approval methods take the digest as an argument.
  Behind the feature `vault-consumer` (dependencies `hpke`, at the version the Worker pins, and `zeroize`),
  `pylota_identity::vault_consumer` is the entry point of § 3.4 of that page: its own `canonicalise_origin` and
  RFC 9421 signer (with `ed25519-dalek`), never a dependency on `core` (the crate rules above; `sdk` is
  Apache-2.0), proven equal to `core::vault::origin::canonicalise` and `core::httpsig::signature_base` by the
  conformance vectors,
  packets are `Zeroizing` buffers, and `Redactor` scans chunked responses. The shared conformance
  vectors are generated from `core`, never from the SDK.
- **Conformance.** `crates/sdk/tests/conformance.rs` plays every case of `sdks/fixtures/conformance/`
  against the client with a fake transport, with `RetryPolicy::standard()` unless a case names another
  policy (the SDK's retries are off by default), as `sdk::conformance::*`.

## 4. In "## 12. Tests", replace the row `xtask::release_sign_sdk_repo` with

| `xtask::release_sign_sdk_repo` | Against recorded draft releases of the three SDK repositories: `release sign --repo` signs each with its own trusted comment (`pylota-identity-typescript v{version}`, `pylota-identity-python v{version}`, `pylota-identity-go v{version}`), refuses provenance naming this repository's workflow, and refuses any other `--repo` |
| `xtask::sdk_vectors_current` | `cargo xtask gen-sdk-vectors --check` passes on a clean tree and fails after a planted change to a vector |
