# S6 Externs and jurisdiction: result

**Pass**, 2026-10-27. Every call went through the `wasm-bindgen` externs in `platform::cf::externs` (kept for
M4), from the Worker `pi-spike-6` (S1's spike Worker, routes `/s6/*`), with the AI binding, the Vectorize
index `pi-spike-6` (1024 dimensions, cosine, metadata index `tenant`) and the AI Gateway `pi-spike-6`. No REST
fallback is needed, so `PM_CF_API_TOKEN` stays optional.

| Call | Result |
|---|---|
| Durable Object ID from `unique_id_with_jurisdiction("eu")`, stored as a string, re-addressed with `id_from_string` | The object reports `ctx.id.jurisdiction = "eu"`; an object from `unique_id()` reports none. FR-PRV-1's basis holds |
| `AI.run("@cf/baai/bge-m3", { text: [2 strings] }, { gateway: { id, collectLog: false, skipCache: true } })` | `{ shape: [2, 1024], data: [[1024 floats], …], pooling: "cls", meta, response }`. The design's checks (`data.len() == n`, 1024 values) hold; the extra fields are ignored |
| `AI.run("@cf/baai/bge-reranker-base", { query, contexts })` | `{ response: [{ id, score }], usage }` with probabilities (0.375 relevant, 0.00008 unrelated): `RerankScore::Probability`, now in [Search](../../docs/src/project/design/search.md) |
| `AI.run("@cf/qwen/qwen3.8-27b", { messages, tools: [{ type: "function", function }] })` | Chat-completions form: `choices[0].finish_reason = "tool_calls"`, `choices[0].message.tool_calls[] = { id, type: "function", function: { name, arguments: "<JSON string>" } }`, `content: null`, plus a `reasoning` string and `usage`. Matches [Search › agentic](../../docs/src/project/design/search.md) |
| `AI.toMarkdown([{ name, blob }])` with a two-page PDF | `{ id, name, format: "markdown", mimeType, tokens, data }`; `data` marks each page `### Page 1`, `### Page 2`. The field is `mimeType`, not `mimetype` ([Rust workspace §7](../../docs/src/project/design/rust-workspace.md#7-wasm-bindgen-externs) corrected); page boundaries are present, so the one-page-per-document fallback is not needed |
| Vectorize `upsert` (4 vectors, 2 namespaces, `tenant` metadata) | `{ mutationId }` |
| `query` with `{ topK, namespace, filter: { tenant }, returnValues: false, returnMetadata: "none" }` | `{ count: 2, matches: [{ id, namespace, score }] }`: only the namespace's vectors that pass the filter |
| `getByIds` | The vectors that exist, with `id`, `namespace`, `metadata` and `values` |
| `deleteByIds` | `{ mutationId }`; `getByIds` stopped returning the vector about 60 s later |
| `describe()` | `{ dimensions: 1024, vectorCount, processedUpToMutation, processedUpToDatetime }`: `IndexInfo` reads `vectorCount` and `dimensions` |

Mutations (upsert and delete) became visible to `getByIds` after about a minute, while `describe()` reported
them as processed within a second. The erasure probe ([Privacy](../../docs/src/project/design/privacy.md))
polls `getByIds`, which is the slower of the two, so its waits stand.

The `gateway` option is honoured: after the gateway `pi-spike-6` was deleted, the same `bge-m3` call failed
with `2001: Please configure AI Gateway in the Cloudflare dashboard`, and succeeded without the option. So a
`PM_AI_GATEWAY` that names a missing gateway fails every AI call; `idvy doctor` must check that the gateway
exists, not only that the variable is set.

## Cleanup

The Worker, the index and the gateway were removed on 2026-10-27.
