Where this goes: `docs/src/project/design/testing.md`. Six changes, each introduced by where it goes ([ADR 0022](../adr/0022-sdk-languages.md), [SDKs, skills and agent tooling § 3](sdk-and-skills.md#3-python-go-and-javascript)).

## 1. In the page's header table, row "Code", replace "the TypeScript SDK's tests live in its own repository ([ADR 0018](../adr/0018-typescript-sdk-in-its-own-repository.md))" with

the TypeScript, Python and Go SDKs' tests live in their own repositories ([ADR 0018](../adr/0018-typescript-sdk-in-its-own-repository.md), [ADR 0022](../adr/0022-sdk-languages.md)); the shared SDK conformance suite is `sdks/fixtures/conformance/` and its Rust runner `crates/sdk/tests/conformance.rs`

## 2. In the layers table, after the row "TypeScript SDK", add these rows

| SDK conformance | `sdk::conformance::` | `crates/sdk/tests/conformance.rs` over `sdks/fixtures/conformance/` | `cargo test --workspace`; `cargo xtask gen-sdk-vectors --check` | The client rules as data, shared by all four SDKs ([SDKs, skills and agent tooling § 3.2](sdk-and-skills.md#32-the-shared-conformance-suite)) |
| Python SDK | `py::`, `py::it::` | The SDK repository `PILOTAAI/pylota-identity-python`, `tests/`: pytest with `respx`, the shared fixtures and the conformance runner | In the SDK repository's CI on CPython 3.11–3.15; `py::it::` against this repository's harness started with `cargo xtask itest --serve`. Names reach `trace` through `sdks/python-tests.json` | The Python SDK and its packaging (FR-SDK-25, FR-SDK-28 – FR-SDK-33) |
| Go SDK | `go::`, `go::it::` | The SDK repository `PILOTAAI/pylota-identity-go`: `go test` with the race detector, the shared fixtures and the conformance runner | In the SDK repository's CI on Go 1.26 and 1.27; `go::it::` against the same harness. Names reach `trace` through `sdks/go-tests.json` | The Go SDK and its release (FR-SDK-26, FR-SDK-28 – FR-SDK-33) |

## 3. In the paragraph that lists the parity live tests (after "`ts::live::round_trip`. The last runs in the TypeScript SDK's own repository, …"), add

`py::live::round_trip` and `go::live::round_trip` run the same way in the Python and Go SDK repositories
([ADR 0022](../adr/0022-sdk-languages.md)).

## 4. In "### 11.1" (the test-name table), after the two `ts::` rows, add

| `py::<module>::<name>` | pytest test of the Python SDK with fakes and shared fixtures, in the SDK repository | `py::cancel::y72_cancellation_propagates` |
| `py::it::<area>::<name>` | pytest test of the Python SDK against this repository's workerd harness, in the SDK repository | `py::it::send::lost_response_one_email` |
| `go::<package>::<name>` | `go test` test of the Go SDK with fakes and shared fixtures, in the SDK repository | `go::cancel::y73_context_done` |
| `go::it::<area>::<name>` | `go test` test of the Go SDK against this repository's workerd harness, in the SDK repository | `go::it::send::lost_response_one_email` |

## 5. In "### 11.2 `cargo xtask trace`", after the sentence ending "…: a missing TypeScript test blocks the SDK release, never the product release.", add

The Python and Go SDKs follow the same rule with their own inventories and assets: `trace` reads `py::` names
from `sdks/python-tests.json` and `go::` names from `sdks/go-tests.json`, prints a name an inventory lacks as
`pending (SDK)`, and every product release attaches `py-tests-required.json` and `go-tests-required.json`
beside `ts-tests-required.json`. `sdk::conformance::` names are ordinary Rust tests of `crates/sdk` and are
checked like any other.

## 6. In "### 11.2", where it says "`sdks/typescript-tests.json` is a list of `{ "name", "covers" }` objects, or an object whose `tests` field is that list.", replace with

`sdks/typescript-tests.json`, `sdks/python-tests.json` and `sdks/go-tests.json` are each a list of
`{ "name", "covers" }` objects, or an object whose `tests` field is that list.

In the same section's prefix list (section 11.1 prefixes, and `xtask/src/trace.rs` `PREFIXES` with M46), add `py`
and `go`.
