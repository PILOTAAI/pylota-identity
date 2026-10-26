Where this goes: `docs/src/project/design/index.md`, row "SDKs, skills and agent tooling".

Replace the row's description with:

| [SDKs, skills and agent tooling](sdk-and-skills.md) | The TypeScript and JavaScript SDK `@pylotahq/identity`, the Python SDK `pylota-identity` and the Go SDK `github.com/PILOTAAI/pylota-identity-go`, each built in its own repository from this design (generated layers over `openapi.yaml`, retries with the same key and bytes, typed outcomes, webhook verification, event and stream helpers, phone and vault helpers, the vault consumer entry point, packaging and publishing), the shared conformance suite all SDKs run, agent skills and plugins checked against the contract, MCP annotations, client hints and the per-key tool allow-list, `idvy mcp stdio`, the CLI's `--schema`, `--query` and `idvy skills generate`, and the framework guides (FR-SDK-2–15, FR-SDK-25–33). |

And in the row "Testing", change "`ts::` for the TypeScript SDK" to "`ts::`, `py::` and `go::` for the SDKs, `sdk::conformance::` for the shared suite".
