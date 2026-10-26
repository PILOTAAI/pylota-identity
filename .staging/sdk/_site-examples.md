For the coordinator and the site writer, not for pasting: what the landing page's SDK examples
(`site/public/index.html`, the tabs `code-rust`, `code-typescript`, `code-python`, `code-go`) must change to match
[SDKs, skills and agent tooling § 3](../../docs/src/project/design/sdk-and-skills.md#3-python-go-and-javascript), checked on 2026-10-26.
FR-SDK-33 then checks each block in its SDK's CI (`ts::doctest::*`, `py::doctest::site_examples`,
`go::doctest::site_examples`), so the page cannot drift again.

| Tab | Today | Change |
|---|---|---|
| Python | `print(sent.id, f"deduplicated={sent.deduplicated}")` | Responses are `TypedDict`s (dictionaries at run time, so unknown fields survive): `print(sent["id"], f"deduplicated={sent['deduplicated']}")` |
| Python | `Client(base_url="https://mail.example.com", api_key=api_key)` | Unchanged; keyword arguments for the body and `idempotency_key=IdempotencyKey(...)` match § 3.5 |
| Go | `client := identity.NewClient("https://mail.example.com", key)` | `NewClient` returns `(*Client, error)`: `client, err := identity.NewClient("https://mail.example.com", key)` followed by `if err != nil { return err }` |
| Go | `identity.IdempotencyKey("bk-2291-confirm")` as the last argument of `SendMessage` | Unchanged: a required-key operation takes the key as an argument, checked at call time |
| Go | import path `github.com/PILOTAAI/pylota-identity-go` | Unchanged; the case must stay `PILOTAAI` (`go::module::path_case`) |
| TypeScript | `sent.id`, `sent.deduplicated` | Unchanged (the response is the message object plus `deduplicated`) |
| Rust | `sent.message.id` | Unchanged (the Rust SDK flattens `allOf` into `SendResponse { message, deduplicated }`, Rust workspace § 11) |
| SDK menu text "Rust, TypeScript, Python and Go" and "Typed clients for Rust, TypeScript and JavaScript, Python and Go" | Unchanged |

Until a package is published, the site must not suggest it can be installed today: the SDK cards link to the
[SDKs](../../docs/src/reference/sdks.md) reference, whose "Status" column says which are published. The launch
plan's go-live gate for the site should check that every SDK the site shows is published (owner's call whether
the site shows unpublished SDKs at Cloud phase C).
