# 0022 SDK languages: Python and Go join Rust and TypeScript

| | |
|---|---|
| Status | Proposed |
| Date | 2026-10-26 |
| Deciders | The owner (TREFT LTD) |
| Related | [SDKs, skills and agent tooling § 3](../design/sdk-and-skills.md#3-python-go-and-javascript), FR-SDK-25 – FR-SDK-33, edge rows Y72–Y92, milestones M46–M48, [ADR 0015](0015-solo-operator.md), [ADR 0018](0018-typescript-sdk-in-its-own-repository.md), [ADR 0020](0020-phone-numbers.md), [ADR 0021](0021-agent-vault.md) |

## Context

Until now the product shipped a Rust SDK (`crates/sdk`, FR-SDK-1) and a TypeScript SDK
(`@pylotahq/identity`, FR-SDK-2, [ADR 0018](0018-typescript-sdk-in-its-own-repository.md)), and
[SDKs, skills and agent tooling § 3](../design/sdk-and-skills.md#3-python-go-and-javascript) deferred a
Python SDK to P2. The owner decided on 2026-10-24 that Python and Go SDKs join them, and that JavaScript is
served by the TypeScript package. The landing page already shows Python (`from pylota_identity import Client`)
and Go (`identity "github.com/PILOTAAI/pylota-identity-go"`) examples.

Three things changed since § 3 was written:

- **Phone numbers and the vault** ([ADR 0020](0020-phone-numbers.md), [ADR 0021](0021-agent-vault.md)) add
  client-side rules that are as easy to break as the mail rules: a phone purchase or an SMS send repeated with
  a new idempotency key spends money twice; an approval sent with a digest the client fetched itself approves
  something a person never saw; and a vault **consumer** (a browser fleet, an egress proxy, an executor) must
  canonicalise origins, sign its requests (RFC 9421, Ed25519) and open HPKE packets exactly as the service
  does. A study of a third-party credential proxy (2026-10-26) found that a consumer comparing origins with its
  own code is where hosts with different case, a trailing dot or a default port slip past an origin check; one
  canonicaliser shipped in every SDK closes that.
- **The integrators** Pylota Identity expects for phone and vault consumers write Python (agent frameworks,
  data pipelines) and Go (proxies, executors) as often as TypeScript.
- **The cost** of a third and fourth SDK for one owner ([ADR 0015](0015-solo-operator.md)) falls if every SDK
  is checked against one shared, language-neutral conformance suite instead of each re-deriving the rules.

Facts the decision rests on, each read on 2026-10-26:

- **Names.** `pypi.org/pypi/pylota-identity/json` and `…/pylota_identity/json` answered `404` (unregistered).
  `proxy.golang.org/github.com/!p!i!l!o!t!a!a!i/pylota-identity-go/@v/list` answered `404` ("not found …
  git ls-remote"): the repository does not exist yet. `registry.npmjs.org/@pylotahq%2fidentity` answered
  `404`; `registry.npmjs.org/-/org/pylotahq/package` answered `200 {}` (the scope exists, with no public
  package).
- **Python lines** (endoflife.date API): 3.10 reached end of life on 2026-10-01; 3.11 (end of life
  2027-10-31), 3.12 (2028-10-31), 3.13 (2029-10-31), 3.14 (2030-10-31) and 3.15 (released 2026-10-09, end of
  life 2031-10-31) are supported.
- **Go releases** (`go.dev/dl/?mode=json`): the stable releases are go1.27.3 and go1.26.10; Go 1.25 reached end
  of life on 2026-08-19 (endoflife.date). The Go 1.26 release notes add "the new crypto/hpke package [that]
  implements Hybrid Public Key Encryption (HPKE) as specified in RFC 9180".
- **Python HPKE.** `cryptography` added HPKE "implementing RFC 9180" in 47.0.0 (2026-04-24, its
  `CHANGELOG.rst`); 50.0.2 (2026-09-30) is the newest release at least two weeks old.
- **Licence display.** pkg.go.dev shows "only limited package and module information" for a module whose
  licence it does not recognise, and states "Currently we cannot add non-OSI approved licenses to the list"
  ([License Policy](https://pkg.go.dev/license-policy)). `FSL-1.1-ALv2` is not on that list. SPDX
  lists `FSL-1.1-ALv2` (`spdx.org/licenses/FSL-1.1-ALv2.json` answered `200`), and the Python packaging guide
  accepts any SPDX expression in `project.license`
  ([Writing your pyproject.toml](https://packaging.python.org/en/latest/guides/writing-pyproject-toml/)).
- **Publishing.** `pypa/gh-action-pypi-publish` supports PyPI trusted publishing with `id-token: write`, and
  "Generating signed digital attestations … is now on by default for all projects using Trusted Publishing"
  (its README at v1.14.2, commit `dc37677b2e1c63e2034f94d8a5b11f265b73ba33`, tagged 2026-07-28). The PyPI
  documentation site (`docs.pypi.org`) could not be reached from this session; its rules are re-read before
  M47. Go modules are published by pushing a tag; the module proxy and checksum database then serve and pin
  the module's hash.

## Decision

1. **Four SDKs, one specification.** Rust (`pylota-identity` on crates.io, `crates/sdk`, this repository),
   TypeScript and JavaScript (`@pylotahq/identity` on npm, `PILOTAAI/pylota-identity-typescript`), Python
   (`pylota-identity` on PyPI, import `pylota_identity`, `PILOTAAI/pylota-identity-python`) and Go (module
   `github.com/PILOTAAI/pylota-identity-go`, repository of the same name). The client rules of
   [SDKs, skills and agent tooling § 2.5–§ 2.14](../design/sdk-and-skills.md#25-idempotency) bind every SDK;
   a language section says only how each rule is expressed in that language.
2. **JavaScript is the TypeScript package.** `@pylotahq/identity` ships ES modules with declarations, works
   from plain JavaScript (the refusals that types enforce are repeated at run time, as FR-SDK-3 already
   requires), and is documented as the JavaScript SDK. There is no second npm package.
3. **Own repositories.** The Python and Go SDKs are built, tested and released from their own public
   repositories, for the reason of [ADR 0018](0018-typescript-sdk-in-its-own-repository.md): this repository
   stays Rust only. The design stays here, as for TypeScript.
4. **Shared conformance.** This repository publishes, in every release's `sdk-fixtures-{version}.tar.gz`, a
   language-neutral `conformance/` directory: scripted HTTP exchanges for the retry, idempotency, outcome,
   pagination, `wait`, event and webhook rules; origin canonicalisation cases; RFC 9421 signature-base
   vectors; HPKE packet vectors; and phone purchase and approval scripts. The Rust SDK runs it here
   (`sdk::conformance::*`); each other SDK runs it in its own CI and refuses to adopt a contract whose
   conformance cases it does not all pass.
5. **Coverage.** Every SDK has one method per operation not marked `x-sdk: false`, and the same helpers:
   outcomes, pagination, `wait` (mail and SMS), webhooks, events, drafts, uploads, the one-call phone purchase
   and phone set-up status, and vault agent and orchestrator calls. Vault **consumer** helpers (origin
   canonicaliser, request signer, packet opener, redeem, report and write-back) live in a separate entry
   point (`@pylotahq/identity/vault-consumer`, `pylota_identity.vault_consumer`, Go package
   `github.com/PILOTAAI/pylota-identity-go/vaultconsumer`, Rust feature `vault-consumer`), never in the default
   import, so a process that drives a model does not load the code that opens packets by accident.
6. **Approvals are never filled in.** No SDK helper approves a phone request, a vault grant or a vault use on
   its own: the generated approval methods take the `digest` as a required argument, and no SDK reads the
   current digest to retry a refused approval.
7. **Runtimes.** Python: every line from its release to its end of life, 3.11 to 3.15 today. Go: the two
   supported releases, 1.26 and 1.27 today; `go.mod` says `go 1.26`, the first release with `crypto/hpke`.
8. **Dependencies.** Go: the standard library, plus `github.com/coder/websocket` v1.8.15 for the event stream
   (it has no dependencies of its own; `go 1.23` in its `go.mod`). Python: `httpx` for HTTP; `websockets`
   behind the extra `events`; `cryptography` behind the extra `vault-consumer`. Python package metadata states
   ranges with a tested floor and an exclusive ceiling at the next minor (`httpx>=0.28.1,<0.29`), because a
   library pinned exactly cannot be installed beside another library with a different pin; the repository's
   `uv.lock` pins every package exactly with hashes, and CI runs the suite at the floor and at the lock.
9. **Licence.** FSL-1.1-ALv2 for the Python and Go SDKs, as for the TypeScript SDK (ADR 0018 decision 2) and
   the product: SPDX expression `FSL-1.1-ALv2` in `pyproject.toml` `project.license`, `LICENSE.md` at the root
   of each repository. Each release becomes Apache-2.0 two years after it ships. Because pkg.go.dev will not
   render documentation for this licence, the Go reference is published with the product docs
   ([SDKs](../../reference/sdks.md)) and `go doc` works locally.
10. **Versions and releases.** Each SDK's major version is the API's path version (`/v1` → 1.x.y) and each
    release names the product release whose contract it was generated from. Each SDK repository's release
    produces its artefacts and an unsigned `SHA256SUMS` with GitHub attestations; the owner signs it offline
    with `cargo xtask release sign <tag> --repo PILOTAAI/pylota-identity-python` or `--repo
    PILOTAAI/pylota-identity-go` (trusted comments `pylota-identity-python v{version}` and
    `pylota-identity-go v{version}`). Python is published by PyPI trusted publishing with attestations; Go by
    pushing the signed release's tag, after the release job checked that the module zip's `h1:` hash equals
    the one in the signed `SHA256SUMS`. No pre-release is published to PyPI and no pre-release tag is pushed to
    the Go repository.
11. **Traceability.** `cargo xtask trace` learns the prefixes `py` and `go`, read from
    `sdks/python-tests.json` and `sdks/go-tests.json` as `ts::` names are read today, and every product
    release attaches `py-tests-required.json` and `go-tests-required.json` beside `ts-tests-required.json`.

## Consequences

- Python and Go integrators, including vault consumers, get the same guarantees as TypeScript ones, and a
  consumer written in any of the four languages compares origins, signs and opens packets with code proven
  against the service's own vectors.
- The landing page's Python and Go examples become real code, checked in CI (FR-SDK-33).
- Harder: four release processes for one owner. The shared conformance suite and the identical release
  steps (attest, sign offline, publish by OIDC or tag) keep each one mechanical; an SDK release never blocks a
  product release, because missing SDK tests are `pending (SDK)` here.
- Go documentation is not on pkg.go.dev while the licence is FSL-1.1-ALv2; the product docs carry it instead.
- Human prerequisites before M47 and M48: create `PILOTAAI/pylota-identity-python` and
  `PILOTAAI/pylota-identity-go` (FSL-1.1-ALv2); register the PyPI project `pylota-identity` with a trusted
  publisher for `release-publish.yml` in environment `release`, which is the act that reserves the name; give
  both repositories a staging test key for their `live::round_trip` tests; confirm the GitHub organisation's
  name stays `PILOTAAI`, because a Go module path is case-sensitive and cannot be renamed without a new
  module.

## Alternatives considered

- **Apache-2.0 (or MIT) for the SDKs only.** Client libraries under a permissive licence install anywhere,
  appear on pkg.go.dev with full documentation and pass licence scanners that refuse non-OSI licences. Not
  chosen, to keep one licence across the product and its clients as ADR 0018 decided; the SDKs contain the
  canonicaliser, signer and packet code that a competing service would otherwise copy. **The owner may
  reverse this** by choosing Apache-2.0 for the TypeScript, Python and Go SDKs at once; that change amends
  ADR 0018 decision 2 and this decision 9 together, and the Rust crate's licence with them if the owner wants
  one rule.
- **Generated clients only, no hand-written layer** (openapi-python-client, oapi-codegen's client
  generator). Fast to start, but each generator owns retries, keys and errors in its own way, which is
  exactly what [§ 2.5–§ 2.8](../design/sdk-and-skills.md#25-idempotency) must control. The generators are used
  for types only.
- **Keep Python at P2 and add no Go SDK.** Python and Go integrators would hand-write clients and vault
  consumers. Not chosen: a hand-written consumer's origin check is the failure the vault study found.
- **One monorepo for the three non-Rust SDKs.** One CI and one release process, but three toolchains in one
  repository and versions that move together whether or not a language changed. Not chosen; the conformance
  suite already gives one source of truth.
