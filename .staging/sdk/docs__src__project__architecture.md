Where this goes: `docs/src/project/architecture.md`, in the repository layout block.

Replace the two `sdks/fixtures` lines with:

```text
sdks/fixtures       recorded HTTP exchanges and the shared SDK conformance suite (conformance/), used by the
                    Rust SDK here and by the TypeScript, Python and Go SDKs, each built in its own repository
                    from each release's openapi.json (ADR 0018, ADR 0022)
```
