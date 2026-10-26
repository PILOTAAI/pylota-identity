# 0018 The TypeScript SDK in its own repository

| | |
|---|---|
| Status | Proposed |
| Date | 2026-10-18 |
| Deciders | The owner (TREFT LTD) |
| Related | [SDKs, skills and agent tooling](../design/sdk-and-skills.md), FR-SDK-2 – FR-SDK-9, [ADR 0001](0001-rust-on-workers.md), [ADR 0015](0015-solo-operator.md), [ADR 0017](0017-agentmail-parity.md), `AGENTS.md` |

## Context

`AGENTS.md` says the product is Rust only: "Do not add TypeScript or JavaScript source files", with one
named exception, the landing page's `site/public/assets/app.js`. [ADR 0001](0001-rust-on-workers.md)
rejected a mixed Rust and TypeScript codebase partly because it breaks that rule, and
[ADR 0005](0005-state-machines.md) rejected a TypeScript sidecar for the same reason.

[ADR 0017](0017-agentmail-parity.md) adds a TypeScript SDK, the npm package `@pylotahq/identity`
(FR-SDK-2 – FR-SDK-9). The first draft of the SDK page put its TypeScript source, an npm lockfile, Biome,
Vitest, a `ts-sdk` CI job, an `npm publish` step and compiled guide examples inside this repository.
Neither ADR 0017 nor that page amended `AGENTS.md`, so a coding agent reading both would get two binding
instructions that contradict each other. The review of pull request #6 found this on 2026-10-18.

Facts the decision rests on:

- Pylota, the first user, is a TypeScript monorepo. Its API server consumes the SDK from npm at an exact
  version, so where the SDK's source lives does not change what Pylota installs.
- The SDK is generated from the published contract (`openapi.yaml`, served as `/openapi.json`) and talks to
  the service only over HTTP. It needs no Rust code and no code of this repository at build time.
- The repository `PILOTAAI/pylota-identity-typescript` did not exist on 2026-10-18 (`gh repo view` answered
  "Could not resolve to a Repository"). The npm scope `pylotahq` exists with no public package
  ([SDKs, skills and agent tooling § 1](../design/sdk-and-skills.md#1-decisions)).

## Decision

1. **The rule stands.** The Rust-only rule of `AGENTS.md` is not changed. This repository gains no TypeScript or JavaScript
   source file, no npm lockfile and no Node.js toolchain for the SDK.
2. **The SDK has its own repository.** `@pylotahq/identity` is built, tested, released and published from the
   public repository `PILOTAAI/pylota-identity-typescript`. The package name stays `@pylotahq/identity`.
   **Amended 2026-10-26 by [ADR 0022](0022-sdk-languages.md) decision 9:** its licence is Apache-2.0 (SPDX
   `Apache-2.0`), in `package.json` `license` and the repository's `LICENSE` and `NOTICE`. It was
   FSL-1.1-ALv2, the same as this repository's, until that decision.
3. **The design stays here.** [SDKs, skills and agent tooling § 2](../design/sdk-and-skills.md#2-the-typescript-sdk),
   FR-SDK-2 – FR-SDK-9, the SDK's edge-case rows and its `ts::` test names remain binding in this
   repository and are the specification the SDK repository implements. A change to the SDK's behaviour is
   written here first, as for any other design.
4. **This repository publishes what the SDK consumes.** Every release attaches three assets, listed in the
   signed `SHA256SUMS`:
   - `openapi.json`, the contract of that release;
   - `sdk-fixtures-{version}.tar.gz`, the recorded HTTP exchanges and error envelopes of `sdks/fixtures/`
     with the Rust SDK's canonical decoding of each;
   - `ts-tests-required.json`, every `ts::` test name the register and the design tables use, with its rows.

   They are JSON data, not code.

   The SDK repository downloads both for one release tag and refuses them unless `SHA256SUMS.sig`
   verifies against the release public keys and every checksum matches.
5. **What moves.** The SDK repository holds the source layout, the generator (in the first draft
   `cargo xtask gen-ts-sdk`), the `ts::`, `ts::it::` and `ts::runtime::` tests, the CI job that runs them,
   the packing, attestation and owner-signed `SHA256SUMS` of its own releases, `npm publish` by trusted
   publishing, and the compiled examples of the framework guides with their lockfile. The guides themselves
   are Markdown and stay in `docs/src/guides/`.
6. **Traceability across the two repositories.** `cargo xtask trace` checks the `ts::` names of the
   edge-case register against `sdks/typescript-tests.json`, a data file the SDK repository's release
   produces (every test title with its `Covers:` line and the SDK version) and a pull request copies here.
   A name the inventory lacks is `pending (SDK)` here, never a failure, so a product release never waits
   for an SDK release; each product release instead attaches `ts-tests-required.json`, and the SDK
   repository refuses to release on a contract whose required `ts::` tests it does not all define
   ([Testing § 11.2](../design/testing.md#112-cargo-xtask-trace)).
   `cargo xtask itest --serve` starts the workerd harness and leaves it running, so the SDK repository's CI
   can run `ts::it::` against the Worker of the contract's tag.

## Consequences

- The Rust-only rule of `AGENTS.md` and "written entirely in Rust" stay true of this repository.
- Nothing changes for Pylota: it installs `@pylotahq/identity` from npm at an exact version, and the package
  still ships the contract it was built from as `@pylotahq/identity/openapi.json`.
- The SDK's version is no longer the product's. Its major version stays the API's path version, and each
  SDK release names the product release whose contract it was generated from (`package.json`
  `pylotaIdentity.contract`). An SDK-only fix no longer needs a product release.
- Harder: two repositories and two release processes for one owner ([ADR 0015](0015-solo-operator.md)).
  An operation reaches the SDK only after a product release publishes it. The SDK's coverage test fails
  when the contract it is generated from has an operation without a method, so a new contract cannot be
  adopted half way.
- Human prerequisites, before M36: create `PILOTAAI/pylota-identity-typescript`; configure npm trusted
  publishing for that repository's release workflow; give that repository a staging test key for
  `ts::live::round_trip`.
- Follow-up outside this record: the browser suite's Playwright specs (Testing § 2) are TypeScript inside
  this repository and are not covered by the `AGENTS.md` exception either. They predate ADR 0017, and the
  owner settles them separately.

## Alternatives considered

- **Amend `AGENTS.md`.** Add an exception for `sdks/typescript/**` and `docs/examples/**`, state that
  Node.js is a build dependency, and add the npm commands to "Commands". This keeps one repository and one
  release, and the SDK always matches the contract of the same commit. It was not chosen because it puts a
  second toolchain and test stack into the product repository, which ADR 0001 rejected, and because a rule
  with a growing list of exceptions stops guiding coding agents. **The owner may reverse this record by
  choosing this alternative.** That change rewrites `AGENTS.md` in the same pull request, returns the SDK
  page's layout to `sdks/typescript/` and restores the `ts-sdk` job in this repository's CI.
- **No TypeScript SDK in v1.0.** Pylota would keep its hand-written client. Not chosen: the SDK exists to
  keep the client-side rules (caller keys, same bytes on retry, `uncertain` never resent, raw-body webhook
  verification) that a hand-written client tends to break.
