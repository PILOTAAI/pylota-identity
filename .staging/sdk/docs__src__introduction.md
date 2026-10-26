Where this goes: `docs/src/introduction.md`. Two changes ([ADR 0022](project/adr/0022-sdk-languages.md)).

## 1. In the first paragraph, replace "The TypeScript SDK is a separate client, built in its own repository from the same contract ([ADR 0018](project/adr/0018-typescript-sdk-in-its-own-repository.md))." with

The TypeScript (and JavaScript), Python and Go SDKs are separate clients, each built in its own repository
from the same contract and proven against the same conformance suite
([ADR 0018](project/adr/0018-typescript-sdk-in-its-own-repository.md), [ADR 0022](project/adr/0022-sdk-languages.md)).

## 2. In the row "**Three ways in**", replace "There is also a Rust SDK and, as P1 features, a TypeScript SDK and agent skills for Claude Code, Codex and Cursor." with

There is also a Rust SDK and, as P1 features, TypeScript and JavaScript, Python and Go SDKs ([SDKs](reference/sdks.md))
and agent skills for Claude Code, Codex and Cursor.
