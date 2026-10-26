Where this goes: `docs/src/project/build-plan.md`. Five insertions, each introduced by where it goes. Milestone
numbers are this design's own (M46–M48); the coordinator renumbers them at integration and keeps the
cross-references (M38 and M39 are the phone milestones, M43 the vault's consumer and ticket milestone). The
milestone text is copied from `docs/src/project/design/sdk-and-skills.md` § Milestone; a difference is a defect.

## 1. In "## Timeline", phase B row: after the vault sentence ("M42–M45 (the vault, …) …"), add

; M46–M48 (the SDK conformance suite and the Python and Go SDKs, [ADR 0022](adr/0022-sdk-languages.md)) follow M36 and the phone and vault milestones whose routes they cover

## 2. In "## Dependency graph", in the second `text` block, add before the line "M20 staging + live proof ─▶ M28 Cloud production commissioning (phases A, B, C)"

```text
M36, M38, M39, M43 ─▶ M46 SDK conformance suite, phone and vault helpers ─▶ M20
   (M46 adds the py and go prefixes to M0's trace and two release assets to M19's release)
M46 ─▶ M47 Python SDK   (in PILOTAAI/pylota-identity-python) ─▶ M20
M46 ─▶ M48 Go SDK       (in PILOTAAI/pylota-identity-go) ─▶ M20
```

## 3. In "**Shared files.**", add this bullet

- `xtask/src/trace.rs` and `xtask` `release`: M36 reads `ts::` names from `sdks/typescript-tests.json` and
  attaches `ts-tests-required.json`; M46 adds the prefixes `py` and `go` with `sdks/python-tests.json`,
  `sdks/go-tests.json`, `py-tests-required.json` and `go-tests-required.json`, and the two SDK repositories to
  `release sign --repo`. Nothing else changes the inventory format.

## 4. New milestones, after the vault milestones (M45) and before "## M20 · Staging deploy and live proof"

## M46 · SDK conformance suite, phone and vault helpers (after M36, M38, M39 and M43)

**Goal.** One language-neutral proof of the client rules, generated from the Rust side, and the phone and vault
helpers in the Rust and TypeScript SDKs, so the Python and Go SDKs are built against data rather than prose.

**Files:** in this repository, `sdks/fixtures/conformance/**`, `crates/sdk/src/{phone.rs, vault.rs,
vault_consumer/{mod.rs, origin.rs, keys.rs, sign.rs, packet.rs, redact.rs}}`, `crates/sdk/tests/conformance.rs`,
`xtask/src/gen_sdk_vectors.rs`, `sdks/python-tests.json` and `sdks/go-tests.json` (empty inventories), the guide
`vault-consumers.md`. In `PILOTAAI/pylota-identity-typescript`: the conformance runner, the phone and vault
helpers and the `vault-consumer` entry point.

| File (owner milestone) | Hook |
|---|---|
| `crates/sdk/Cargo.toml`, `crates/api-types/Cargo.toml` (M16, M0) | `license = "Apache-2.0"` in place of the workspace licence, with `LICENSE` and `NOTICE` in each crate; `deny.toml` allows `Apache-2.0` for these two ([ADR 0022](adr/0022-sdk-languages.md) decision 9) |
| `crates/sdk/Cargo.toml` (M16) | Feature `vault-consumer` with `hpke` (the version the Worker pins), `zeroize` |
| `xtask/src/trace.rs` (M0) | Prefixes `py` and `go`, read from `sdks/python-tests.json` and `sdks/go-tests.json` as `ts::` names are; a missing name is `pending (SDK)` |
| `xtask` `release` (M19) | Assets `py-tests-required.json` and `go-tests-required.json`; `release sign --repo` accepts `PILOTAAI/pylota-identity-python` and `PILOTAAI/pylota-identity-go` with their trusted comments |
| `.github/workflows/ci.yml` (M19) | `cargo xtask gen-sdk-vectors --check` in the `test` job |

**Implements:** FR-SDK-28 – FR-SDK-31 and rows Y76–Y85, Y89 (the Rust and TypeScript parts).

**Acceptance:** `sdk::conformance::*`, `xtask::sdk_vectors_current`, the extended `xtask::release_sign_sdk_repo`;
in the TypeScript repository `ts::conformance::all`, `ts::package::y80_consumer_entry_point_separate`,
`ts::it::vault::y78_consumer_signature_accepted` and `ts::it::phone::y82_buy_retry_one_number`; the reference
consumer of [Vault § 24.3](design/vault.md#243-consumer-conformance-fixtures) built on the Rust helpers.

## M47 · Python SDK (after M46)

**Files:** the repository `PILOTAAI/pylota-identity-python` ([SDKs § 3.5](design/sdk-and-skills.md#35-python)); here, `sdks/python-tests.json`
(the inventory a release produces) and the guide `python-sdk.md`.

**Implements:** FR-SDK-25, FR-SDK-32 and FR-SDK-33 for Python, and rows Y72, Y75, Y80, Y86, Y87, Y89, Y91,
Y92 (Python parts).

**Human prerequisites.** Create `PILOTAAI/pylota-identity-python` (Apache-2.0); register the PyPI project
`pylota-identity` with a trusted publisher (repository `PILOTAAI/pylota-identity-python`, workflow
`release-publish.yml`, environment `release`), which reserves the name; disallow API tokens on the project;
give the repository a staging test key.

**Acceptance:** the `py::` rows above pass in the SDK repository's CI on CPython 3.11–3.15 and are listed in
`sdks/python-tests.json`; `py::conformance::all`; the first release publishes the sdist and wheel listed in its
owner-signed `SHA256SUMS`, with PyPI attestations; `py::doctest::site_examples`; M20 gains
`py::live::round_trip`.

## M48 · Go SDK (after M46)

**Files:** the repository `PILOTAAI/pylota-identity-go` ([SDKs § 3.6](design/sdk-and-skills.md#36-go)); here, `sdks/go-tests.json`, the guide
`go-sdk.md` and the Go section of the [SDKs](../reference/sdks.md) reference.

**Implements:** FR-SDK-26, FR-SDK-32 and FR-SDK-33 for Go, and rows Y73, Y74, Y80, Y86, Y88, Y90, Y91, Y92
(Go parts).

**Human prerequisites.** Create `PILOTAAI/pylota-identity-go` (Apache-2.0) with an environment `release`
whose token may push tags; give it a staging test key; keep the organisation name `PILOTAAI`.

**Acceptance:** the `go::` rows above pass on Go 1.26.10 and 1.27.3 and are listed in `sdks/go-tests.json`;
`go::conformance::all`; the first release's tag is pushed only after the owner's signature, and the proxy's
`h1:` equals the signed one; `go::doctest::site_examples`; M20 gains `go::live::round_trip`.

## 5. In "## M20 · Staging deploy and live proof", in the step that lists the parity live tests

After "a TypeScript SDK round trip (`ts::live::round_trip`, M36);" add "Python and Go SDK round trips
(`py::live::round_trip`, M47; `go::live::round_trip`, M48), each run by its SDK repository with a staging
test key;". In the same milestone's acceptance list, if the parity live tests are listed by name, add the two
names there too.
