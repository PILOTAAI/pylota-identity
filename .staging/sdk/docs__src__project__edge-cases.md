Where this goes: `docs/src/project/edge-cases.md`. Three changes. Rows are copied from
`docs/src/project/design/sdk-and-skills.md` § Edge cases ([ADR 0022](adr/0022-sdk-languages.md)); a difference between
the two is a defect.

## 1. In "## Y · Deliverability metrics and agent tooling", after row Y71, add

| Y72 | A Python call is cancelled (`asyncio.CancelledError`, `KeyboardInterrupt`) during a keyed send, before or after the request left | The exception propagates unwrapped; no retry and no new key; the caller's later retry with the same key returns the first result if the server stored one | S+I | `py::cancel::y72_cancellation_propagates` |
| Y73 | A Go call's `ctx` is cancelled or its deadline passes during a keyed send | The call ends at once with an error wrapping `ctx.Err()` and, for a keyed request that may have been sent, `*OutcomeUnknownError` with the key; no retry after `ctx` is done | S+I | `go::cancel::y73_context_done` |
| Y74 | A Go SDK older than the deployment decodes new fields and enum values | Decoding never fails; unknown enum values are kept and `IsKnown()` is false; `WithRawResponse` returns the exact bytes | S | `go::compat::y74_unknown_values_tolerated` |
| Y75 | A Python SDK older than the deployment decodes new fields and enum values | Unknown keys stay in the returned dictionary; an unknown enum value is a plain `str`; nothing raises | S | `py::compat::y75_unknown_values_tolerated` |
| Y76 | An SDK repository adopts a contract whose conformance cases it does not all pass, or the vectors are stale | The SDK's release refuses; in this repository `cargo xtask gen-sdk-vectors` changing a file fails CI | S | `sdk::conformance::all`, `xtask::sdk_vectors_current`, `ts::conformance::all`, `py::conformance::all`, `go::conformance::all` |
| Y77 | A consumer canonicalises `HTTPS://TFL.GOV.UK.:443/`, an `xn--` label, a Unicode host, `http://`, userinfo, an IP literal in any notation, port 80 or `localhost` | Every SDK returns the same canonical origin or the same refusal reason as `core::vault::origin::canonicalise` | S | `sdk::conformance::vault_consumer_origins`, `ts::conformance::all`, `py::conformance::all`, `go::conformance::all` |
| Y78 | A consumer signs a request with a clock 5 minutes off, retries it, or sends a body re-serialised after signing | `created` comes from the server clock learnt from `Date`; a retry signs again with a new nonce and the same bytes; `Content-Digest` is over the bytes sent; the service accepts each | S | `sdk::conformance::vault_consumer_signatures`, `ts::it::vault::y78_consumer_signature_accepted` |
| Y79 | A packet is opened with the wrong encryption key, for another use, phase or origin (`aad`), or with one byte changed | Opening fails with no partial plaintext; the opened packet's `use_id`, `phase` and `origin` must equal the request's; closing a packet overwrites its fields | S | `sdk::conformance::vault_consumer_packets`, `py::vault_consumer::y79_packet_close_overwrites`, `go::vaultconsumer::y79_packet_close_overwrites` |
| Y80 | An agent process imports the SDK's default entry point | The vault consumer helpers and the packet code are not loaded; they need their own import (and, in Python, the `vault-consumer` extra; in Rust, the feature) | S | `py::package::y80_consumer_not_in_default_import`, `ts::package::y80_consumer_entry_point_separate`, `go::package::y80_consumer_package_separate` |
| Y81 | A redemption's response is lost after the request was sent | No retry: the call ends in `OutcomeUnknownError` and the orchestrator creates a new use; a `503 unavailable` envelope with `retryable: true` is retried with the same ticket | S+I | `sdk::conformance::vault_consumer_redeem`, `py::conformance::all` |
| Y82 | The one-call phone purchase is retried after a timeout, meets a taken number, or needs a person | The retry carries the same key and returns the first purchase; one number; `pending` and `taken` are typed results with the request or the alternatives; no purchase of an alternative without the caller | S+I | `sdk::conformance::phone_buy`, `ts::it::phone::y82_buy_retry_one_number` |
| Y83 | `wait_for_sms` is retried after a network error, or the client clock runs ahead | One `since`, fixed from the server clock before the first attempt, on every attempt; an SMS that arrived during the retry is returned | S | `sdk::conformance::phone_sms_wait` |
| Y84 | An SMS becomes `uncertain`, or has a status the SDK does not know | `classify` returns `uncertain` or `unknown_status`; no SDK code path sends it again | S+I | `sdk::conformance::phone_sms_outcomes` |
| Y85 | An approval of a phone request, vault grant or vault use is sent with a stale digest | `409` surfaced with `details.current_digest`; the SDK never re-reads the object and approves the newer digest; an approval without a digest is refused before any request | S | `sdk::conformance::approvals_need_digest` |
| Y86 | The PyPI token store or the Go repository's tags are compromised, or the proxy serves different module content | PyPI accepts uploads only from trusted publishing in environment `release`, of files in the signed `SHA256SUMS`; the Go tag is pushed after signing and the proxy's `h1:` must equal the signed one, else the owner is paged and the version is retracted | S | `py::release::y86_pypi_files_in_signed_sums`, `go::release::y86_module_hash_matches_signed` |
| Y87 | A Python application resolves `httpx`, `websockets` or `cryptography` to the oldest version the metadata allows | The suite passes at every floor; the metadata excludes the next minor | S | `py::compat::dependency_floors` |
| Y88 | The Go module is imported as `github.com/pilotaai/pylota-identity-go`, or a doc or example spells it so | The proxy refuses the other case; every example, doc and the site use `PILOTAAI` exactly | S | `go::module::path_case` |
| Y89 | A Python or Go webhook handler is given a parsed body (`dict`, a decoded struct) | Python raises `TypeError`; Go's `Verify` takes only `[]byte`; the vectors show a re-serialised body fails | S | `py::webhooks::y89_raw_body_only`, `sdk::conformance::webhooks` |
| Y90 | The Python or Go event stream meets every close code, `410 cursor_expired` and a revoked key | The rules of Y34–Y37 hold in each language | S | `py::conformance::all`, `go::conformance::all` |
| Y91 | A key, stream token or vault ticket could leak through `repr`, `str`, `pickle`, logging, `fmt` verbs, `slog` or an error | It never appears: shown as prefix and lookup only; pickling a Python client raises `TypeError` | S | `py::secrets::y91_key_never_serialised`, `go::secrets::y91_key_never_formatted` |
| Y92 | The Python SDK runs under Pyodide (`sys.platform == "emscripten"`), or the Go SDK is built for `GOOS=js` | Construction is refused with `ConfigError` `browser_not_supported` | S | `py::secrets::y92_browser_refused`, `go::secrets::y92_browser_refused` |

## 2. In the preamble

- Add 21 to the row count and to the count of rows Pylota Identity owns (all 21 are `S` or `S+I`); the
  integrator-owned count is unchanged. Other designs staged in the same pull request change these numbers too:
  compute the totals once, after every block is pasted.
- After the `ts::` bullet, add:

- `py::` and `go::` tests are tests of the Python and Go SDKs, defined here and run in their own repositories,
  `PILOTAAI/pylota-identity-python` and `PILOTAAI/pylota-identity-go` ([ADR 0022](adr/0022-sdk-languages.md)),
  with the shared fixtures and conformance suite of `sdks/fixtures/`; `py::it::` and `go::it::` tests run
  there against this repository's local workerd (`cargo xtask itest --serve`), and `trace` checks their names
  against `sdks/python-tests.json` and `sdks/go-tests.json`;
- `sdk::conformance::` tests run the shared conformance suite against `crates/sdk`, the Rust SDK from which
  its expected values are generated;

## 3. In row Y20, Test cell

Append ", `py::it::send::lost_response_one_email`, `go::it::send::lost_response_one_email`" after
`ts::it::send::y20_lost_response_one_email`.
