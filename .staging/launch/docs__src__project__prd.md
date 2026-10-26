Where this goes: `docs/src/project/prd.md`. Three insertions, each introduced by where it goes. Requirement IDs are this staging's own; renumber at integration only if another design took the same ID.

## 1. In "### 6.13 Operations", add after FR-OPS-5

- **FR-OPS-6** Pylota Identity Cloud **must** go live product by product (mail, phone numbers, the vault,
  the SDKs) only through the gates of [Cloud commissioning](cloud-commissioning.md#10-go-live-gates), each
  opened when every row of [Launch readiness](launch-readiness.md) that blocks it is `done` or `n/a` with
  evidence: build milestones, production checks, legal pages and processors, counsel and tax answers, licence
  obligations, security sign-off and operations. `release-gate` **must** refuse `v1.0.0` while a row that
  blocks `v1.0` is open. Acceptance: `xtask::launch_readiness_refuses_open_blockers`,
  `cli::doctor::launch_readiness_gates`.

## 2. In "## 7. Non-functional requirements", add these rows after NFR-CON-1

| NFR-REL-5 | Inbound SMS accepted → webhook delivered | p95 ≤ 30 s, p99 ≤ 120 s |
| NFR-REL-6 | Provider webhook acknowledged (`/hooks/telnyx`) | p99 ≤ 500 ms |
| NFR-PERF-7 | Vault ticket redemption, consumer request to sealed packet | p95 ≤ 500 ms (excluding a one-time-code long poll) |
| NFR-SEC-3 | Vault values in API responses, MCP results, events, webhook deliveries or logs | 0 |
| NFR-OPS-4 | A product flag turned on with a row that blocks its gate still open | 0 ([Launch readiness](launch-readiness.md)) |

## 3. In "## 9. Release criteria (v1.0)", add after criterion 7

8. Every row of [Launch readiness](launch-readiness.md) whose **Blocks** names `v1.0` is `done` or `n/a`
   with its evidence (FR-OPS-6).
