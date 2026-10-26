Where this goes: `docs/src/reference/limits.md`, section "## SDKs and agent tooling".

Replace the four rows that begin "TypeScript SDK" with these, so the limits read for every SDK
([SDKs, skills and agent tooling § 3.1](../project/design/sdk-and-skills.md#31-one-specification-four-sdks)):

| SDK attempt timeout (TypeScript, Python, Go) | 30 s (1–120 s); waits: their `timeout` + 15 s | Pylota Identity |
| SDK retries (TypeScript, Python, Go; the Rust SDK's retries are off unless `RetryPolicy::standard()`) | 3 (0–10), backoff 0.5 s, 1 s, 2 s + up to 250 ms; `Retry-After` slept up to 60 s (1–300 s); `request_in_progress` 5 more attempts | Pylota Identity |
| SDK `collect` | 10,000 items by default | Pylota Identity |
| SDK deduplication lease | 60 s by default (15–3,600 s) | Pylota Identity |
| Vault consumer request signature | `expires` 30 s after `created`; `created` from the server clock | [Vault § 8.4](../project/design/vault.md#84-authenticating-a-consumer) |
