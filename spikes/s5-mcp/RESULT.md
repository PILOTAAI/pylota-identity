# S5 MCP over Streamable HTTP: result

**Pass**, 2026-10-27, with the local protocol types of [ADR 0009](../../docs/src/project/adr/0009-local-mcp-protocol-types.md)
(no `rmcp`).

The Worker `pi-spike-5` serves `POST /mcp` from `fetch` through `platform::export_worker!`, with one tool,
`echo`, and the dual-era rules of [MCP §2](../../docs/src/project/design/mcp.md): `initialize` selects the 2025
revisions; a request whose `_meta` carries `io.modelcontextprotocol/protocolVersion` is served statelessly
under 2026-07-28.

| Client | Result |
|---|---|
| MCP Inspector 2.12.0 (`--cli --transport http`) | `tools/list` returned `echo`; `tools/call` returned `hello-s5`. It used the legacy era (`initialize`; results without `resultType`) |
| Claude Code 2.1.312 | `claude mcp list`: `√ Connected`. A headless prompt allowed only `mcp__s5__echo` called the tool and answered `claude-code-s5` |
| `curl`, modern era | `server/discover` returned `supportedVersions`, `capabilities` and `resultType: "complete"`; a `tools/call` with matching `MCP-Protocol-Version`, `Mcp-Method` and `Mcp-Name` succeeded; a mismatched `Mcp-Method` returned `400` with `-32020`; `GET` returned `405` |

The Worker never starts a tokio runtime or timer: `tokio` is in its wasm graph only through `worker` 0.8.7,
as for `crates/worker`. Bundle: 5 ms startup.
