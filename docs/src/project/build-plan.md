# Build plan

This is the order of work for building Pylota Identity from an empty repository to v1.0, written for a
coding agent (or a team) working through it in one pass. Each milestone lists:

- the files it creates;
- the requirements it implements;
- the tests that prove it;
- the gate that must be green before the next milestone starts.

Design documents are binding. When a milestone shows a design is wrong, stop, write an ADR, update the
design, then continue. Never let code and docs drift. When two pages disagree, `openapi.yaml` wins for
wire behaviour and the design page wins for internal behaviour
([Design › Precedence](design/index.md#precedence)).

## How to work through it

1. **Test first.** For each item, write the test named in the [edge-case register](edge-cases.md) or the
   milestone's acceptance list, watch it fail, then implement.
2. **Gate after every milestone:**

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo xtask build-worker   # wasm build + size budget
   cargo xtask itest          # from M5 onwards
   mdbook build docs
   ```

3. **One pull request per milestone** (or per track once tracks run in parallel). The PR description
   lists the FR IDs and edge rows covered. `main` is protected. Nothing merges red.
4. **Contracts are frozen** once written: API paths and shapes, error codes, event types, MCP tool names,
   CLI commands. Additive changes need a docs update in the same PR. Breaking changes need an ADR.
5. **Record spike outcomes** in the design document they affect, under a "Spike result" note with the date.

## Timeline

| Phase | What | Elapsed time with AI coding agents |
|---|---|---|
| A. Foundation and spikes | M0–M1 | 0.5–1 day. Spikes need a real Cloudflare account and DNS |
| B. Build | M2–M19, M21–M27 and M29–M48, run as parallel tracks after M5; M29–M37 (the AgentMail-parity features, [ADR 0017](adr/0017-agentmail-parity.md)) follow the core tracks they extend; M38–M41 (phone numbers, [ADR 0020](adr/0020-phone-numbers.md)) follow M5, M6, M8, M14, M17 Foundation, M21, M22, M27, M30 and M31, and M41 also the vault's core milestone; M42–M45 (the vault, [ADR 0021](adr/0021-agent-vault.md)) follow M5, M6, M14, M15, M16, M17 Foundation, M21, M25, M27, M30 and M31; M46–M48 (the SDK conformance suite and the Python and Go SDKs, [ADR 0022](adr/0022-sdk-languages.md)) follow M36 and the phone and vault milestones whose routes they cover; M49 (launch) follows M28 | 3–5 days of agent time, depending on parallelism and review speed |
| C. Live proof | M20: staging deploy, live end-to-end suite, deliverability checks | 2–5 days. DNS propagation, Email Sending onboarding and real-mailbox tests are wall-clock bound |
| D. Hardening before production traffic | DMARC ramp (`p=none` → `quarantine` → `reject`), Postmaster Tools enrolment, external review | 4–6 weeks of calendar time, mostly waiting, run in parallel with early use |
| E. Cloud production | M28: commission `pylotamail.com` and open it in three gated phases, partner-only, waitlist, then open ([Cloud commissioning](cloud-commissioning.md)); M49: launch, opening phone numbers, the vault and the SDKs through gates P, V and S once their rows of [Launch readiness](launch-readiness.md) are clear | Phase A within a day of M20; phase B at least 14 days later; phase C at least 30 days after B, alongside phase D |

Writing the code is the fast part. The time that cannot be compressed is the live proof:

- real inbound from Gmail and Outlook;
- bounces and complaints;
- a domain change;
- a domain on an external DNS host;
- a domain failure with fallback;
- erasure with probes.

None of this is optional. Each check guards against a failure mode that has already happened in
production with the previous provider.

## Dependency graph

An arrow means "must be done before".

```text
M0 skeleton ─▶ M1 spikes ─▶ M2 core ─▶ M3 api-types ─▶ M4 platform ─▶ M5 worker base
                                                                        │
            ┌─────────────────────────────┬──────────────┬──────────────┼───────────────┐
            ▼                             ▼              ▼              ▼               │
     M6 identities                  M16 SDK/CLI    M17 observability  M19 site/release  │
     and outbox                                                                         │
            ├──────────────┐                                                            │
            │              ▼                                                            │
            │        M8 webhooks                                                        │
            │              │                                                            │
            ▼              │                                                            │
     M7 inbound ◀──────────┘                                                            │
     and wait                                                                           │
            │                                                                           │
     ┌──────┴───────┬───────────────┐                                                   │
     ▼              ▼               ▼                                                   │
 M9 outbound   M10 search      M12 triage                                               │
     │              │               │                                                   │
     ▼              ▼               │                                                   │
 M13 domains   M11 agentic          │                                                   │
     │              │               │                                                   │
     └──────┬───────┴───────┬───────┘                                                   │
            ▼               ▼                                                           │
     M14 privacy      M15 MCP ◀─────────────────────────────────────────────────────────┘
            │               │      (M15 also waits for M22 and M25, second graph)
            └───────┬───────┘
                    ▼
            M18 quality gates ─▶ M20 staging + live proof ─▶ v1.0
```

The console, billing, domain-method, agent-key and notification milestones, the two halves of M17, and
the AgentMail-parity milestones M29–M37 join the graph like this. Each also feeds M20:

```text
M7, M8, M9, M10, M11, M12, M13, M14 ─▶ M21 console ─▶ M22 billing ─▶ M24 cloud sign-up and sign-in
   (the console's pages show what these build)        (M22 also needs M9 and M12)
M9 outbound ─▶ M13 domains ─▶ M23 domains on any DNS host   (S10, S11, S12 gate its methods)
M5, M6 ─▶ M25 agent signing keys, assertions and signed requests ─▶ M15 MCP (two signing tools)
                                                                  ─▶ M21 console (keys on the identity page)
   (S13 gates signed HTTP requests only)
M22 billing ─▶ M15 MCP (mail_get_usage calls M22's GET /v1/usage)
M9, M10, M21, M22, M24 (and M6's system identity) ─▶ M26 notifications and usage alerts
M16 SDK/CLI ─▶ M19 release pipeline (a release ships the CLI binaries; the site half of M19 needs only M5)
M5 ─▶ M17 Foundation (metrics writer, alert evaluator, alert table, re-seal sweep) ─▶ M7, M8, M9
   (M17 Completion, the checks that measure later milestones, is accepted at M20)
M7, M8, M14, M15, M16, M21 ─▶ M27 service sign-up ledger ─▶ M20
   (M27 also adds two tools to M15's table and its commands to M16's CLI)
M8, M9, M14, M15, M16, M21, M22, M24, M26 ─▶ M29 drafts, approvals and scheduled send ─▶ M20
   (M29 also adds six tools to M15's table and its commands to M16's CLI; the upload arm of
    drafts/attachments.rs lands with whichever of M29 and M33 comes second)
M8, M14, M15, M16, M17 Foundation, M21 ─▶ M30 event feed and live stream ─▶ M20
   (M30 replaces M5's TenantEvents stub, adds the hibernation glue to M4's platform crate, a tool to M15 and
    commands to M16)
M7, M8, M12, M14, M15, M16, M17 Foundation, M21, M26 ─▶ M31 screening hold ─▶ M20
   (M31 adds Q_SCREEN to M5's template if M5 has merged, a tool to M15 and commands to M16)
M7, M8, M9, M14, M15, M16, M21 ─▶ M32 calendar invitations ─▶ M20   (S1 records the raw send)
M7, M9, M14, M15, M16, M22 ─▶ M33 uploads ─▶ M20
   (M33 adds two tools to M15's table, commands to M16's CLI, and a draft-save hook shared with M29)
M24, M26, M27, M29, M30, M31, M33 ─▶ M34 agent self-sign-up ─▶ M20
   (M34 adds four tools to M15's table and its commands to M16's CLI)
M9, M13, M14, M15, M16, M21 ─▶ M35 deliverability metrics ─▶ M20
   (M35 moves M9's pause evaluation behind core::abuse first, adds two tools to M15 and commands to M16)
M8, M15, M16, M19 ─▶ M36 SDKs, skills and agent tooling ─▶ M20
   (its event helpers after M30, draft helpers after M29, upload helpers after M33)
M7–M16, M21, M23, M27 ─▶ M37 parity additions ─▶ M20 (or the next staging candidate once M20 has passed)
   (M37 adds hooks to M29's drafts and M30's feed when those have landed)
M5, M6, M8, M14, M17 Foundation, M21, M22, M27 ─▶ M38 phone numbers, requests and compliance ─▶ M20
   (M38 adds Q_PHONE to M5's template and three QuotaRequest variants, both named exceptions; four tools to
    M15 and commands to M16; the standing budget and one-call purchase land here)
M38, M30, M31 ─▶ M39 SMS ─▶ M20   (its vault code arm lands with whichever of M39 and the vault core comes second)
M38 ─▶ M40 voice routes and calls ─▶ M20
M38, M39, M40, vault core ─▶ M41 porting, port-out guard, reconciliation, invoicing and migration ─▶ M20
   (production keeps PM_PHONE off until M41 is accepted)
M5, M6, M14, M17 Foundation, M27 ─▶ M42 vault core: keys, items, audit, erasure ─▶ M20
   (spikes S-VLT1 and S-VLT2 first; M42 adds the vault steps to M14's jobs and six permissions to M5's router)
M42, M15, M16, M21, M25 ─▶ M43 grants, uses, approval, consumers, tickets ─▶ M20
   (two Decision arms in M5's auth/people.rs; six tools to M15; the conformance fixtures in crates/conformance)
M43, M30, M31 ─▶ M44 one-time codes and sessions ─▶ M20   (its SMS arm lands with whichever of M44 and M39 comes second)
M44 ─▶ M45 rotation, signing, reveal, tooling and the Pylota cut-over ─▶ M20
   (production keeps PM_VAULT off until M45 is accepted)
M36, M38, M39, M43 ─▶ M46 SDK conformance suite, phone and vault helpers ─▶ M20
   (M46 adds the py and go prefixes to M0's trace and two release assets to M19's release)
M46 ─▶ M47 Python SDK   (in PILOTAAI/pylota-identity-python) ─▶ M20
M46 ─▶ M48 Go SDK       (in PILOTAAI/pylota-identity-go) ─▶ M20
M20 staging + live proof ─▶ M28 Cloud production commissioning (phases A, B, C; gates P, V, S as their milestones land)
M28, M41, M45, M48 ─▶ M49 launch (every row of Launch readiness done or n/a for the gates opened)
```

After M5, these tracks can run in parallel, each in its own branch and worktree:

- **Track 1:** M6 → M7 → M9 → M13 (domains need the send path for fallback sends and `transport/ses.rs`);
  M7 also waits for M8, whose `webhooks/payloads.rs` builds the `message.*` events that inbound mail emits;
- **Track 2:** M8, once M6 lands (M8 imports M6's `webhooks/envelope.rs`: the event envelope, the
  `WebhookJob` queue message and the identity payload builders that M6's outbox already needs);
- **Track 3:** M17 Foundation first (it must merge before M7, M8 and M9), then M16 and M17 Completion;
- **Track 4:** M19 (its site half from M5; its release half once M16 has landed);
- **Track 5:** M25, once M6 lands (it needs only M5 and M6).

Once M7 lands, M10 and M12 also run in parallel with M9. M23 follows M13 on the domains track. M21
starts only after M7–M14 and M25, because its screens (inboxes, search, quarantine, triage, domains,
webhooks, erasure, identity keys) call their services; M22 starts after M9, M12 and M21, M24 after M21 and
M22, and M26 after M9, M10, M21, M22 and M24 (it sends through M6's system identity, is fed by M8's
webhook dispatcher and M12's triage, which come before M21, and adds hooks to M24's sign-in files).
M15 also waits for M22, whose `GET /v1/usage` its `mail_get_usage` tool calls, and for M25, whose two
signing tools it registers. M27 comes after M7, M8, M14, M15, M16 and M21, because its gate changes M7's
ingest and `wait`, its events need M8's fan-out, its rows join M14's erasure and retention jobs, it adds two
tools to M15 and commands to M16, and its page joins M21's console. M29 to M36 come after the milestones
named in their headings, for the reasons each gives; M34 also waits for M29, M30, M31 and M33, whose
permissions, events, policy fields and routes its provisional overlay restricts. M37 comes after M7 to
M16, M21, M23 and M27, because each of its nine additions extends one of them (the list in its hooks
table); its additions are independent and can land as separate pull requests. All nine come before M20,
so the live proof covers them. The phone milestones M38–M41, the vault milestones M42–M45 and the SDK
milestones M46–M48 also come before M20, in the order of their headings: M41 needs the vault core M42 for
the port-out PIN, M39 and M44 share the SMS code arm, and M46 needs the phone and vault routes it wraps. M28
runs against production after M20, and M49 (launch) after M28, M41, M45 and M48.
M17 is accepted in two halves, without renumbering: **M17 Foundation** (the
metrics writer, the alert evaluator, the alert table and the master-key re-seal sweep) is Track 3's first
pull request and merges before M7, M8 and M9, because M7 and M8 emit their SLI metrics through its writer
and M9's G3 (`it::ops::provider_quota_80`) and M23's N26 (`ses_identities_90pct`) fire through its
evaluator; **M17 Completion** (J3, every metric emitted, the SLOs) is accepted at M20, because it
measures what later milestones build. No milestone depends on one that comes later in this graph.
Tracks never edit the same files. Shared files (`router.rs`, `wrangler.toml` template, `0001_init.sql`)
are changed only by the track that owns them, as listed per milestone.

**Shared files.** These files are changed by milestones that can run at the same time, so they have a
rule of their own:

- `quota/mod.rs`, the `TenantQuota` object. M5 creates it with every `QuotaRequest` variant already
  answered by a stub (below), and declares every type the interface uses (`Feature`, `BillingMode`,
  `Allowances`, `Outcome`, `CapScope`, `QuotaWarning`, `Held`, `Denied`, `AllowanceRow` and one answer
  type per request), all written out in [Outbound › TenantQuota](design/outbound.md#tenantquota), so
  later milestones replace the behaviour of the variants they own and never change a signature: M7
  (`AdmitInbound`), M9 (`Reserve`, `Release`, `RecordOutcome`), M11 (`CountAgentic`), M12
  (`CountTriageModel`), M22 (the allowance variants, through
  `billing/quota.rs`), M24 (`OutcomeRates`, `SystemMail`), M26 (the `NotifierRequest::UsageThreshold`
  hook), M31 (`CountScreenModel`) and M34 (`SystemMailClass::Signup`, `CountSignupOutcome`). The one
  exception is M35, which adds `GetRates` and the `thresholds` field of `RecordOutcome`
  ([Deliverability metrics § 3.3](design/metrics.md#33-one-source-of-truth)). M38 is a second exception: it adds
  `PhoneSpendHold`, `PhoneSpendSettle` and `PhoneSpendRelease` and the counters `phone_spend:*`,
  `phone_spend:reg:*`, `phone_budget:t:*` and `phone_search` ([Phone numbers § 18.3](design/phone-numbers.md#183-spend-caps),
  [§ 5.7](design/phone-numbers.md#57-the-standing-purchase-budget)).
- `consumers/index.rs`, the `pm-index` consumer. M7 creates it for attachment text, M10 adds chunking,
  embedding and reconciliation, M12 the triage job, and M31 the `Skip` of embedding and triage for a
  message in `screening`. M7 writes the whole `IndexJob` enum
  ([Search § 6](design/search.md#6-indexing-pipeline-pm-index)), so each later milestone fills in only the
  arm of its own job kind.
- `billing/webhook.rs`, the Stripe webhook handler. M22 creates it; M24 adds the `ramp_lifted_at` updates
  (set when a workspace's paid plan is paid for, cleared when a dispute opens), and M26 the
  `Account { event: payment_failed }` hook.
- `jobs/erasure.rs`, tenant and person erasure. M14 creates it with every step and the named stubs of its
  table (below); M21 (console rows), M22 (`cancel_billing` and the billing rows), M23 (the
  `pm-retired-{n}` entries), M24 (person deletion), M26 (the Notifier rows) and M27 (the
  `service_accounts` rows) each fill in their own stub; M29 (the draft rows and `drafts_deleted`), M30 (the
  event-log steps), M31 (the screening receipt counts), M33 (the upload rows and `uploads_deleted`), M34
  (the `agent_signups` rows and the person scrub) and M37 (`list_entries_deleted` and the identity-scope
  `sender_lists` rows) each add their own steps, and M38 (the `phone_release` step, the compliance-document
  steps and the phone receipt fields), M39 (the SMS rows), M40 (the call rows) and M42 (the vault steps of
  identity and tenant erasure, custody slots first, and the receipt fields `vault_items_deleted`,
  `vault_versions_shredded`, `vault_keys_shredded`, `vault_uses_deleted`, `vault_audit_deleted`).
- `jobs/retention.rs`, the global retention job. M14 creates it with the job framework and the steps whose
  tables have writers by then; M21 (`console`), M22 (`billing_events`), M23 (`ses_ingest`), M24
  (`signup`), M25 (`identity_keys`), M27 (`service_accounts`), M29 (draft purge and pending-approval expiry)
  and M30 (the event-log steps), M38 (phone requests, provider events,
  the message index, charges, KYC retention and onboarding drafts), M39 (SMS verification rows) and M42 (the
  `vault` step: terminal grants and uses after 90 days, audit by age after 400 days with anchors, revoked
  consumers after 400 days) each add their own step and its test. If M23 or M25 lands before
  M14, M14 writes that step, and the milestone's test runs once M14 has landed.
- `deploy/wrangler.toml.tmpl` and the `export_worker!` call in `crates/worker/src/lib.rs`. M5 writes
  both whole: every binding of [Configuration › Bindings](../reference/configuration.md#bindings) (the
  seven Durable Object namespaces, the queues, the nine `RL_*` bindings, Vectorize, Workers AI, Email
  Sending, Analytics Engine and the crons) and all seven Durable Object classes in the one `v1` migration tag,
  each class a stub until its milestone: an `ObjectApp` that answers `Init` by storing the owner and
  everything else with `internal_error`. `TenantQuota` is M5's own stub (below). Later milestones replace
  a stub class and add consumers and cron jobs; none adds a binding or a class: `IdentityMailbox` in M6,
  `JobRunner` a stub in M6 and real in M14, `DomainMonitor` and `SesControl` in M13 (`SesControl`'s SES
  calls in M23), `Notifier` in M26, `TenantEvents` in M30. M5's `TenantEvents` stub also exports the three
  WebSocket hibernation handlers, each closing with `1011`, so that M30 changes no export. The one
  exception to "no binding" is M31: if M5 merged before `Q_SCREEN` was listed, M31 adds `Q_SCREEN`, the
  `pm-screen` consumer and `pm-screen-dlq`. M38 is the second exception: it adds `Q_PHONE`, the `pm-phone`
  consumer, `pm-phone-dlq` and the R2 lifecycle rule for `phone-staging/`. A queue whose consumer is not built yet has no producer either,
  so it receives nothing.
- `crates/core/src/sealed.rs`, the registry of columns sealed under `PM_MASTER_KEY`. M17 Foundation
  creates it with the columns written by then (`signing_keys.ciphertext`); each milestone that writes a
  sealed column adds its entry and its case in `it::secrets::master_key_rotation`: M8 (webhook secrets),
  M23 (SMTP credentials), M24 (second factors, the pending enrolment secret and PKCE verifiers), M25
  (identity keys) and M37 (webhook delivery headers, `webhook_endpoints.headers_sealed`), M38
  (`phone_tenants.sip_sealed`, `phone_compliance_profiles.tax_ids_sealed` and `representative_sealed`,
  `phone_compliance_documents.key_sealed`, `phone_requests.secrets_sealed`), M40
  (`phone_voice_connectors.api_key_sealed`, `phone_voice_routes.server_secret_sealed`) and M42
  (`vault_keys.private_sealed` and `vault_custody_slots.private_wrapped`, whose inner layer the sweep carries
  unopened). M25 is the only
  one that can land before M17 Foundation; if it does, M17 Foundation adds its entry.
- `drafts/attachments.rs`: its upload arm is written by whichever of M29 and M33 lands second, with
  `it::drafts::q6_upload_references` and `it::uploads::q108_draft_reference`.
- `mailbox/ingest.rs` and `core::thread`: M7 creates them; M32 adds the `UID` step of thread resolution.
  `core::compose`: M9 creates it; M32 adds the iMIP structure. M44 adds quarantine rule 4b and the
  `vault_code` reason to `mailbox/ingest.rs`; `mailbox/codes.rs` gains `VaultTakeCode`.
- `console/pending.rs`, the pending sign-in step: M24 creates it; M34 adds the `two_factor` step with
  `agent_signup_id`.
- `auth/people.rs`: M38 adds `Decision::ApprovePhoneRequest`, `Decision::DecidePortOut` and
  `Decision::SetPhoneBudget`; M43 adds `Decision::ApproveVault` and `Decision::ActivateConsumer`.
- `auth/routespec.rs`: M42 adds `Auth::ConsumerSignature` for the five consumer routes (wired in M43) and the
  six `vault:*` permissions with the identity-level refusals of
  [Vault § 17.2](design/vault.md#172-what-each-kind-of-key-may-hold).
- `xtask/src/trace.rs` and `xtask` `release`: M36 reads `ts::` names from `sdks/typescript-tests.json` and
  attaches `ts-tests-required.json`; M46 adds the prefixes `py` and `go` with `sdks/python-tests.json`,
  `sdks/go-tests.json`, `py-tests-required.json` and `go-tests-required.json`, and the two SDK repositories to
  `release sign --repo`. Nothing else changes the inventory format.

Each change to one of these files has one owner, the milestone that needs it. When two tracks change the
same file at once, the one that merges second rebases onto the other and re-runs the gate; neither edits
the other's arm.

---

## M0 · Repository skeleton and CI

**Files:** `Cargo.toml` (workspace, `[workspace.dependencies]` pinned per
[Rust workspace](design/rust-workspace.md)), `rust-toolchain.toml`, `crates/{core,platform,api-types,worker,sdk,cli,conformance}/`,
`xtask/`, `.github/workflows/ci.yml`, `deny.toml`, `.cargo/config.toml`, `migrations/d1/`,
`deploy/wrangler.toml.tmpl`.

**Implements:** the workspace and dependency rules in `AGENTS.md`, and the size check of NFR-SEC-2.

### Human prerequisites

A coding agent cannot create these. A person provides each one before the milestone or spike that
needs it, and stores the credential under the name in the last column. Local values go in the shell or in
`spikes/.env` (git-ignored); CI values are GitHub Actions secrets on `PILOTAAI/pylota-identity`, in the
environment named in brackets. Nothing in this table is ever committed.

| Item | Who provides it | Needed by | Secret or config name |
|---|---|---|---|
| Production: Pylota's existing Cloudflare account, on the Workers Paid plan ("shared, tightened", [ADR 0010](adr/0010-cloud-in-the-existing-cloudflare-account.md)). Staging and the spikes: a **separate** Cloudflare account on the Workers Paid plan, because setup uses fixed resource names (`pylota-identity`, `pylota-identity-blobs`, `pm-*`) and production's tokens list only production's zones | Owner (TREFT LTD) | M1 (every spike except S10 and S13, in the staging account), M20 | `CLOUDFLARE_ACCOUNT_ID` (local); `PM_CF_ACCOUNT_ID` (written by `idvy setup`); Actions: `STAGING_CLOUDFLARE_ACCOUNT_ID` [`staging`] |
| The `pylotamail.com` zone in the production account (bought 2026-10-09, [Cloud sign-up §2](design/cloud-signup.md#2-hostnames)); in the staging account, a staging platform zone apex and a **staging tenant zone** (a second apex, listed in the live suite's test tenant's `domains.cloudflare_zones`, for M20 step 4's zone subdomain and zone apex) | Owner | M1 (S2, S7, S9 use a scratch zone or the staging zones), M20 | `PM_PLATFORM_DOMAIN`, `PM_API_HOST`, `PM_CONSOLE_HOST` in `deploy/wrangler.toml`; Actions: `STAGING_TENANT_ZONE` [`staging`] |
| The setup API tokens, with the permissions and scopes in [Deploy › step 2](../self-hosting.md#2-create-a-cloudflare-api-token): a short-lived first-run token with Workers Admin at product scope (it creates the Worker and its Custom Domain, then is deleted), then a deploy token with per-Worker Editor on `pylota-identity` and specific zones only; and the Worker's token, limited to named zones (on production `pylotamail.com` and `pylota.io`, with `domains.allow_create_zone` off) | Owner | M1, M20 | `CLOUDFLARE_API_TOKEN` (local, used by `idvy` and Wrangler); `PM_CF_API_TOKEN` (Worker secret, a separate token with the "Worker token" permissions of that table, stored by the operator with `idvy wrangler secret put`, [Deploy › Domains on Cloudflare](../self-hosting.md)); Actions: `STAGING_CLOUDFLARE_API_TOKEN` [`staging`] |
| A separate Cloudflare account for the nightly evaluations (Workers Paid), and an API token on it with exactly Workers AI · Read, Workers AI · Edit and Vectorize · Edit ([Testing § 9.2](design/testing.md#92-running)) | Owner | M18 | Actions: `PM_EVAL_CF_ACCOUNT_ID` and `PM_EVAL_CF_API_TOKEN` (repository secrets). Never the production account: Vectorize permissions are account-wide |
| A Cloudflare Enterprise account (optional) | Owner, through Cloudflare sales | S10 only | `S10_CLOUDFLARE_ACCOUNT_ID`, `S10_CLOUDFLARE_API_TOKEN` in `spikes/.env`. **S10 may be skipped:** without it `delegated_subdomain` stays off (`PM_CF_SUBDOMAIN_SETUP=off`) and the spike result says "skipped, no Enterprise account" |
| Two AWS accounts with SES production access in `eu-west-2` (London, decided 2026-10-09), on the à la carte plan: one for production and one for staging and the spikes. One region of one account holds one receiving deployment (`idvy setup ses` refuses a second, [Domains on any DNS host §4.2](design/domain-connections.md#42-deployment-set-up-for-ses)) | Owner; production access is requested in the AWS console and approved by AWS, which can take a day | S8, S11, then M23 and M20 step 5 | `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`, or `AWS_PROFILE` (local); `PM_SES_ACCESS_KEY_ID` and `PM_SES_SECRET_ACCESS_KEY` (Worker secrets, written by `idvy setup ses`); Actions: `STAGING_AWS_ACCESS_KEY_ID`, `STAGING_AWS_SECRET_ACCESS_KEY` [`staging`] |
| Two real SMTP submission providers (for example a Google Workspace mailbox and a Microsoft 365 mailbox), each with a sending account on a test domain | Owner | S12 | `S12_SMTP_A_HOST`, `S12_SMTP_A_USERNAME`, `S12_SMTP_A_PASSWORD`, and the same for `S12_SMTP_B_*`, in `spikes/.env` |
| Gmail (Google Workspace) and Microsoft 365 test mailboxes holding only synthetic mail, with API access for the harness | Owner | M20 (the live suite) | Actions: `STAGING_GMAIL_CLIENT_ID`, `STAGING_GMAIL_CLIENT_SECRET`, `STAGING_GMAIL_REFRESH_TOKEN`, `STAGING_M365_TENANT_ID`, `STAGING_M365_CLIENT_ID`, `STAGING_M365_CLIENT_SECRET` [`staging`] |
| A domain at an external DNS provider (not Cloudflare) and that provider's API token | Owner | M20 step 5 (`live::domains::dns_records_external_host`) | Actions: `STAGING_EXTERNAL_DNS_TOKEN`, `STAGING_EXTERNAL_DOMAIN` [`staging`] |
| A staging platform key (90-day expiry) | Created by the agent with `idvy keys create` on staging; stored by a person | M20 | Actions: `STAGING_PLATFORM_KEY` [`staging`] |
| A Stripe account in test mode | Owner | M22 (recorded fixtures), M20 step 11 | `PM_STRIPE_SECRET_KEY` (a restricted key, `rk_test_…`) and `PM_STRIPE_WEBHOOK_SECRET` (Worker secrets on staging); Actions: `STAGING_STRIPE_SECRET_KEY`, `STAGING_STRIPE_WEBHOOK_SECRET` [`staging`]; `STRIPE_SETUP_KEY` (a test-mode restricted key on the owner's machine for `cargo xtask stripe-setup`, [Billing › Stripe account setup](design/billing.md#stripe-account-setup)) |
| A Google OAuth client and a GitHub OAuth app, with redirect URLs on the staging and production console hosts | Owner | M24 (its gate re-reads both providers' documentation), M20 step 10 | `PM_OAUTH_GOOGLE_CLIENT_ID`, `PM_OAUTH_GITHUB_CLIENT_ID` (variables); `PM_OAUTH_GOOGLE_CLIENT_SECRET`, `PM_OAUTH_GITHUB_CLIENT_SECRET` (Worker secrets) |
| The minisign release key pair, generated offline by the owner (`minisign -G`) | Owner | M19 | Secret key: never in GitHub or any online store; kept on an encrypted removable drive (two copies) with its password in the owner's password manager, and used only by `cargo xtask release sign` on the owner's machine ([Security § 11](design/security.md#11-supply-chain)). Public key: compiled into `idvy` as the `current` key ([CLI and setup §8.2](design/cli.md#82-signature-and-checksums)); a second key pair becomes `next` before the first rotation |
| The GitHub environment `production` (deployments from `main` only; the owner is the only person who can start `cloud-deploy.yml`) | Owner | M19 (site deploy); the Cloud rollout after v1.0 | Actions [`production`]: `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` for `site.yml`, a token with the per-Worker Editor role on `pylota-mail-site` and Workers Routes · Edit on the `pylotamail.com` zone, nothing else (the header of `.github/workflows/site.yml`); `PROD_CLOUDFLARE_API_TOKEN` and `PROD_CLOUDFLARE_ACCOUNT_ID` for `cloud-deploy.yml`, a token with the per-Worker Editor role on `pylota-identity`, D1 Edit and Queues Edit, no zone, created for one rollout with a 24-hour expiry ([Cloud commissioning §3](cloud-commissioning.md)). The account is shared with Pylota, so neither token may reach Pylota's Workers or zones. Immutable releases turned on in the repository settings (M19) |
| Terms of service, privacy notice and data processing agreement for Pylota Identity Cloud, published by TREFT LTD | Owner (TREFT LTD) | M24 (sign-up does not start without them), M20 step 10 | `PM_TERMS_URL`, `PM_PRIVACY_URL`, `PM_DPA_URL` and `PM_TERMS_VERSION` (variables on staging and production) |
| Stripe live mode for TREFT LTD: business details, bank account and identity checks done, Stripe Tax on, and the UK VAT registration date | Owner | Cloud launch after M20 (no milestone waits for it) | `STRIPE_SETUP_KEY` (a live restricted key on the owner's machine, for `cargo xtask stripe-setup --live --vat-from <date>`); `PM_STRIPE_SECRET_KEY` and `PM_STRIPE_WEBHOOK_SECRET` (Worker secrets on production) |
| The Email Sending daily quota that the Cloudflare dashboard shows for the account shared with Pylota, less Pylota's own daily volume in that account ([Cloud sign-up § 10.3](design/cloud-signup.md#103-shared-domain-breaker)) | Owner | M24 (system-mail budgets and the shared-domain breaker), M20 | `PM_DAILY_SEND_QUOTA` (variable on staging and production); updated when Cloudflare raises the quota |
| Optional: registration of the production deployment's Web Bot Auth key directory (`https://{PM_API_HOST}/.well-known/http-message-signatures-directory`) with Cloudflare's verified-bot programme (dashboard, "Bot Submission Form", verification method "Request Signature"; [Deploy › Signed HTTP requests](../self-hosting.md#signed-http-requests-web-bot-auth)) | Owner, after M25 ships with S13 passed and `PM_WEB_BOT_AUTH=on` | No milestone or test: signatures verify for any Web Bot Auth verifier without it, and S13 expects the unregistered `401` | None (a dashboard form; nothing to store) |
| The GitHub repository `PILOTAAI/pylota-identity` (created 2026-10-09), with Actions enabled, the environments `staging` (the owner as its one required reviewer, with "Prevent self-review" left off, which is GitHub's default, so the owner approves the runs they start; `main` and `v*` tags only), `release` (`v*` tags only) and `ops` (`main` only; the heartbeat's keys), and branch protection on `main`. GitHub applies environment protection rules on Free, Pro and Team plans only to public repositories ([Managing environments for deployment](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/manage-environments), read 2026-10-10), so the repository is public or on a plan that has them | Owner | M0 | Actions: `CARGO_REGISTRY_TOKEN` [`release`], for `cargo publish`; `HEARTBEAT_KEY` and `HEARTBEAT_MAIL_TEST_KEY` [`ops`], created on production after M20 ([Observability § 5.5](design/observability.md#55-alert-email-and-the-external-heartbeat)); every other secret above |

A milestone whose prerequisite is missing stops and reports which row is missing. It never substitutes a
fake for a spike's real provider.

**Acceptance:**

- `cargo test --workspace` passes on an empty test per crate.
- `cargo xtask build-worker` builds a "hello" Worker to wasm. The `worker` attribute macros are never
  used in `crates/worker` ([Rust workspace §2](design/rust-workspace.md#2-crate-responsibilities-and-allowed-dependencies)),
  so M0 writes a minimal `platform::export_worker!` that exports only a `fetch` answering `200`; spike S1
  validates its glue against the real runtime, and M4 extends it to the other handlers and the Durable
  Object classes. The size check runs and passes, and from here on enforces NFR-SEC-2 on every pull
  request: the compressed bundle stays ≤ 10 MiB (`xtask::size_budget`).
- CI runs these jobs: fmt, clippy, native tests, wasm build, `cargo deny check` (with `deny.toml`'s
  `[graph] targets` and licence exception of [Security § 11](design/security.md#11-supply-chain)),
  `mdbook build docs`, and `trace`, which reads the `MILESTONES` file M0 creates with the line `M0`
  ([Testing § 11.2](design/testing.md#112-cargo-xtask-trace)), so it checks only landed milestones.
- A CI check fails if any crate other than `platform` depends on `worker`
  (`cargo xtask check-layering`), proved by `xtask::check_layering_rejects_worker_dep`.

---

## M1 · Spikes (each one gates design choices)

Run against a scratch Cloudflare account and zone. Each spike is a small program under `spikes/` (not
shipped), plus a written result.

| Spike | Prove | Pass criteria | If it fails |
|---|---|---|---|
| S1 Bindings smoke | From Rust with `worker` 0.8.7, through the `platform::export_worker!` entry glue (which replaces `#[event]` and `#[durable_object]`, see [Rust workspace](design/rust-workspace.md#2-crate-responsibilities-and-allowed-dependencies)): receive an email event and read `from`, `to`, headers and the raw stream; send with the `send_email` binding's structured `send()` including `replyTo`, `headers` (`In-Reply-To`, `References`, `Auto-Submitted`, `X-*`) and attachments (`attachment` and `inline` with `contentId`); produce and consume Queues with `delay_seconds` and `retry_with_options`, and confirm that `Message::timestamp()` is unchanged across retries; DO SQLite with the `transactionSync` extern (a thrown error rolls back) and alarms; D1 `batch`; a multi-statement request to the D1 query API (`POST /accounts/{a}/d1/database/{id}/query`); and the response of the local `wrangler dev` email endpoint to a `setReject` ([Testing §6.4](design/testing.md#64-injecting-inbound-mail)); and a raw `EmailMessage` send of a calendar message (`multipart/alternative` with `text/plain` and `text/calendar; method=REQUEST; charset=UTF-8`) to two arbitrary recipients: whether it is accepted, whether one `EmailMessage` can carry several envelope recipients, whether the `text/calendar` part and its `method` parameter arrive unchanged, and whether Cloudflare keeps the composed `Message-ID` ([Calendar invitations §5.6](design/calendar-invitations.md#56-transports)) | Every call works from Rust. The returned `messageId` is captured. A rolled-back transaction leaves no rows. A multi-statement D1 query-API request is atomic: when its last statement fails, none of the earlier statements' rows remain | Raw MIME send (`EmailMessage`) built with `mail-builder` for any missing structured field. If the entry glue cannot replace the macros: an ADR allowing exactly one file, `crates/worker/src/entry.rs`, to use them ([Rust workspace §2](design/rust-workspace.md#2-crate-responsibilities-and-allowed-dependencies)). If the D1 query API is not atomic: every migration file is made re-runnable and a CI lint enforces it ([CLI and setup §8.5](design/cli.md#85-d1-migrations)). **`transactionSync` has no fallback; this is an accepted risk.** The planned path is a `wasm-bindgen` call to `ctx.storage.transactionSync`, which Cloudflare documents with no restriction on the calling method beyond a SQLite-backed object ([SQLite storage API](https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/), read 2026-10-09), so any JS method of the class, including the glue's, may call it. If S1 shows otherwise, the build stops and an ADR is written before M4 continues. The owner accepted this risk on 2026-10-09. Calendar sends on the `cloudflare` transport stay refused with `422 transport_unavailable` (`calendar_needs_raw_mime`); with one recipient per raw send, they are limited to one recipient (`calendar_single_recipient`). |
| S2 Inbound failure semantics | What the sending MTA sees when `email()` throws, versus `setReject` (documented as a permanent error); which `Authentication-Results` headers reach the handler | Throwing yields a 4xx temporary failure and the sender retries. The exact SMTP reply text for both cases is recorded. Record which `Authentication-Results` authserv-id Cloudflare stamps on delivered mail (setup later writes it to `PM_TRUSTED_AUTHSERV_ID`, [Inbound › Authentication verdict](design/inbound.md#authentication-verdict)) | **Throw only.** The handler keeps its in-handler R2 retries (three attempts) and then throws, as designed, whatever the sender is shown; the spike result records the observed reply in [Inbound](design/inbound.md#interface). There is no `forward()` to a backup address: setup registers no Email Routing destination address ([Identities and domains › Cloudflare API token](design/identity-domains.md#cloudflare-api-token-permissions)), so none exists to forward to |
| S3 FTS5 in DO SQLite | `content='', contentless_delete=1`, the `trigram` tokenizer, `bm25()` with six column weights, `DELETE FROM fts WHERE rowid = ?`, and renaming an FTS5 table (for the index swap in [Search](design/search.md)) | All work on the deployed runtime, not only on local workerd | External-content table `fts_docs` ([Data model](design/data-model.md)); disable trigram and rely on reference normalisation plus semantic fallback; without rename, the swap rebuilds `fts` in place |
| S4 Wasm budget | Bundle size, cold start, CPU time and peak memory when parsing and verifying a 25 MiB message and a 40 MB message from the SES source ([N5](edge-cases.md)), and `mail-auth` verdict parity on the corpus | Compressed bundle ≤ 10 MiB (NFR-SEC-2), cold start under 1 s, both messages parsed under 128 MB peak and inside the CPU limit, DKIM and DMARC verdicts equal the reference implementation on every corpus message | Write attachments to R2 before parsing bodies; move heavy features behind cargo features; switch the release profile to `opt-level = "z"` |
| S5 MCP over rmcp | Streamable HTTP served from `fetch` using `rmcp` 3.4.1 protocol types, without a tokio runtime. Note: `rmcp` 3.4.1 (like 3.5.1) declares `tokio` (features `sync`, `macros`, `rt`, `time`) as a non-optional dependency (crates.io metadata, read 2026-10-10) | MCP Inspector and Claude Code connect, list tools and call one; the wasm build never starts a tokio runtime or timer, and the bundle stays inside the S4 budget | Implement the JSON-RPC types locally in `worker`; keep `rmcp` as a native dev-dependency for client tests. **Taken in advance** ([ADR 0009](adr/0009-local-mcp-protocol-types.md)): M1 still runs S5 against the local types to record the Inspector and Claude Code result |
| S6 Externs and jurisdiction | Vectorize `upsert`, `query` (namespace and metadata filter), `deleteByIds`, `getByIds` and `describe()` (the vector count); `AI.run` with the `gateway` option, including the `bge-m3` output shape, the reranker's score form and the agent model's chat-completions schema ([Search](design/search.md)); `AI.toMarkdown` (including whether PDF output marks page boundaries) — all through `wasm-bindgen` externs; DO IDs from `unique_id_with_jurisdiction("eu")` stored as strings and re-addressed with `id_from_string` | Every call works and each recorded shape matches the design, or the design is updated with the observed one. An EU object reports the EU jurisdiction (`ctx.id.jurisdiction`) | REST fallbacks (`/vectorize/v2/…`, `/ai/run`, `/ai/tomarkdown`) using `PM_CF_API_TOKEN`, which then becomes required ([Rust workspace §7](design/rust-workspace.md#7-wasm-bindgen-externs)); one page per document when `toMarkdown` does not mark pages. If an EU object does not report the EU jurisdiction, the build stops for an owner decision, because FR-PRV-1 depends on it |
| S7 Outbound Message-ID | The relationship between the `messageId` that `send()` returns and the `Message-ID` header recipients see | Either a deterministic mapping (strategy A), or the header learned from a journal copy (strategy B) | Strategy B: a hidden journal BCC to `journal+{message ulid}.{identity ulid}@{PM_PLATFORM_DOMAIN}`; the email handler records the header and drops the copy ([Outbound](design/outbound.md#message-id-of-outbound-mail-spike-s7)). If a journal copy never arrives, that message matches replies by thread token and provider ID only |
| S8 SES in wasm | SigV4 signing for SES v2 `SendEmail` with raw content, and SNS message signature verification (`SignatureVersion` 2; version 1 is refused), from Rust in wasm | A real send through SES in `eu-west-2`; a real SNS notification verified, and a tampered one rejected | SES leaves v1.0: an ADR moves `send_only`, `dns_records`, `smtp_relay` with `inbound: ses` and the SES failover to v1.1 |
| S9 Event subscriptions and onboarding APIs | Create an Email Sending event subscription to `pm-delivery-events` through the API for one domain (source type `email.sending` with `zone_id` and `domain`; this source shape appears in Wrangler's source, not yet in the API reference), receive all six event types, delete it. Onboard a zone **apex** and a subdomain through `POST /zones/{zone_id}/email/sending/subdomains`, and enable routing on a subdomain through `POST /zones/{zone_id}/email/routing/dns` with `name`. A literal routing rule whose `worker` action value is the script name `pylota-identity` delivers to the Worker. **Subdomain enable and disable** in a zone whose apex already routes, and in one whose apex has a foreign MX: enabling a subdomain leaves the apex's MX records and catch-all unchanged; `PATCH /zones/{zone_id}/email/routing/dns` with the subdomain's `name` unlocks its records, deleting them by ID stops mail to that subdomain only, the apex and another subdomain keep receiving, and the zone's mail-domain count drops; `DELETE /zones/{zone_id}/email/routing/dns` disables the whole zone (recorded, never used for one name). Record the error codes Cloudflare returns for a zone deleted out of band, for a 31st mail domain in a zone, and for a zone that already exists on `POST /zones` | Payload fields match [Outbound › Delivery events](design/outbound.md#delivery-events); subscriptions can be created per domain at runtime with `PM_CF_API_TOKEN`; apex and subdomain onboarding both work through the API; a subdomain's routing can be removed without touching the zone's other mail domains | For `cloudflare_zone` and `nameservers`, the API creates the domain without a subscription, marks it `delivery_events: "manual"`, and returns its records with `details.action = "run idvy domains subscribe <domain>"`; delivery events start once that command has run ([Identities and domains › Kind `zone`](design/identity-domains.md#kind-zone), tests `it::domains::s9_manual_delivery_events` and, for `nameservers`, `it::domains::s9_manual_delivery_events_nameservers`). Any onboarding step the API cannot do is listed by `idvy domains add` as a dashboard step and checked by `idvy doctor`. If a subdomain's routing cannot be removed on its own, removal leaves it with no rules and `idvy doctor` lists it under `routing.leftover_subdomains` with the dashboard step ([Identities and domains › Domain removal](design/identity-domains.md#domain-removal)) |
| S10 Child zones | On an Enterprise account, a subdomain-setup child zone accepts Email Routing catch-all to the Worker and Email Sending onboarding, and both work end to end. Optional: skipped when no Enterprise account is available ([Build plan › Human prerequisites](#human-prerequisites)) | Mail to any address at the child apex reaches `email()`; a send is DKIM-aligned | `delegated_subdomain` stays off; `dns_records` covers the case |
| S11 SES receiving | Rule set, S3 action and topic as specified; the notification shape, including the `objectKey` form; S3 `GetObject` with SigV4 from a Worker; a 39 MB message ([N5](edge-cases.md)); `user+tag@` routing; the retired-address bounce; the backstop picks up a message whose push failed | All pass in `eu-west-2` | `dns_records` and `smtp_relay` with `inbound: ses` do not ship in v1.0; `send_only` still does |
| S12 SMTP from a Worker | Ports 465 and 587 with `StartTls` against two real providers; the certificate host name is checked (a wrong-name certificate is refused); timeouts and the uncertain window behave as designed | All pass | `smtp_relay` does not ship in v1.0 |
| S13 Web Bot Auth format | A request signed by `core::httpsig` with the deployment key (`Signature-Agent` as a quoted structured-field string; `Signature-Input` covering `@authority`, `signature-agent` and `from`, with `tag="web-bot-auth"`, `keyid` = the JWK thumbprint, `created`, `expires` and a 64-byte nonce), sent to `https://crawltest.com/cdn-cgi/web-bot-auth`, which answers `401` for a well-formed message with an unknown key, `200` for a known key that verifies and `400` otherwise ([Web Bot Auth](https://developers.cloudflare.com/bots/reference/bot-verification/web-bot-auth/), read 2026-10-09). Needs no Cloudflare account | `401` before the key directory is registered (well-formed, unknown key), never `400` | Signed HTTP requests stay off in v1.0: `PM_WEB_BOT_AUTH` cannot be turned on ([Agent signing keys](design/agent-keys.md#5-signed-http-requests-web-bot-auth)). Agent assertions are unaffected |

**S1 writes the entry glue.** S1 needs the `email`, `queue`, `scheduled` and Durable Object exports
(constructor, `fetch`, `alarm`) that the M0 glue lacks, so S1 writes the full `platform::export_worker!`
and `platform::cf::glue` in `crates/platform` (not under `spikes/`), with one throwaway Durable Object
class. M4 keeps that code and hardens it: the `WorkerApp` and `ObjectApp` traits, error handling, fakes
and tests.

**Gate:** every spike has a written result. Design documents are updated where a fallback was taken.

---

## M2 · Core logic (`crates/core`, no I/O)

**Files:** `crates/core/src/{ids.rs, address.rs, thread_token.rs, thread.rs, reply.rs, mime/, sanitize.rs, text.rs, quote.rs, refs/, classify.rs, auth.rs, trust.rs, attach.rs, query/, fusion.rs, citations.rs, triage_rules.rs, policy.rs, dns.rs, domain_fsm.rs, injection.rs}`,
`crates/conformance/corpus/`, `fuzz/`.

**Implements:** the parsing and decision logic behind FR-ADR-6/7, FR-IN-3, 6, 7 and 9, FR-THR-1,
FR-SRCH-3/4/8 (verifier), FR-TRI-2, FR-DOM-4/5 (the pure state machine).

**Acceptance:**

- Unit tests for every `core::` row in sections A–H of the edge-case register (section N's are in M23):
  A1–A4, A12, B2, B4–B11, B13, C1, C2,
  C8, D1–D3, D6, D8, D9, E1, F1, F5, F9, F11, H2, H3, H7. (D10's test is `it::inbound::d10_token_bruteforce`,
  which needs the mailbox's `rate_windows`, so it belongs to M7.)
- A property test for the query parser: every input parses or returns `invalid_query`, and every
  compiled FTS expression contains only quoted terms.
- A property test for thread tokens: round-trip works, and any bit flip fails verification.
- A conformance corpus of at least 300 messages, covering Gmail, Outlook, Apple Mail, Thunderbird,
  mailing lists, DSNs (RFC 3464), MDNs, calendar, TNEF, S/MIME, PGP, charsets and malformed input.
  Each message has an expected JSON output.
- Fuzz targets `mime_parse`, `query_parse`, `address_parse`, `sanitize` and `dsn_parse` each run for
  60 seconds in CI without a crash.
- `crates/core` builds for `wasm32-unknown-unknown`.

---

## M3 · API types and OpenAPI

**Files:** `crates/api-types/src/{lib.rs, errors.rs, objects/*.rs, requests/*.rs, events/*.rs, openapi.rs}`.

**Implements:** FR-API-1/2, plus the types for every object in [REST API](../reference/api.md) and
[events](../reference/events.md).

**Acceptance:**

- `cargo test -p pylota-identity-api-types` generates `openapi.json` with `utoipa`, and a test compares it
  semantically (paths, methods, schemas, enums, required fields) with `docs/src/reference/openapi.yaml`.
  Any difference fails.
- Every error code in [Errors](../reference/errors.md) exists in the `ErrorCode` enum with its HTTP
  status and `retryable` flag, checked by a table test.
- Serde round-trip tests for every object, using the examples in `openapi.yaml`. The examples in `api.md`
  are not used: they elide fields (`"…"`), so they are not complete objects.

---

## M4 · Platform crate

**Files:** `crates/platform/src/{lib.rs, clock.rs, rng.rs, d1.rs, durable.rs, r2.rs, queues.rs, ai.rs, vectorize.rs (extern), email.rs, ratelimit.rs, dns.rs (DoH), http.rs, tcp.rs, metrics.rs (Analytics Engine), background.rs (waitUntil), config.rs, fakes/, cf/glue.rs}`
(the glue S1 wrote, hardened).

**Implements:** the trait set and the `Platform` bundle in [Rust workspace and platform](design/rust-workspace.md),
with Cloudflare implementations and in-memory fakes for native tests; `WorkerApp`, `ObjectApp`,
`HttpRequestIn` and `CfPlatform` (§6.3); `export_worker!` for the `fetch`, `email`, `queue` and
`scheduled` handlers and seven Durable Object classes (with the three WebSocket hibernation handlers of
`TenantEvents`); `Config` with the startup rules (§6.1).

**Acceptance:**

- Each trait has a fake used by native tests in `worker` logic modules.
- `platform::config::startup_rules` (every row of the startup rules) and
  `platform::ids::monotonic_within_ms`.
- The DoH resolver parses the JSON answers from both configured resolvers. A test uses canned
  responses for TXT, MX, NS and CNAME, including NXDOMAIN and SERVFAIL.
- Only this crate depends on `worker`, enforced by `check-layering`.

---

## M5 · Worker base: routing, auth, tenants, keys

**Files:** `crates/worker/src/{lib.rs, router.rs, auth.rs, errors.rs, ratelimit.rs, request_id.rs, keyring.rs, rpc.rs, log.rs, metrics.rs, handlers/{meta.rs, tenants.rs, partners.rs, keys.rs, audit.rs}, db/{mod.rs, tenants.rs, partners.rs, keys.rs, audit.rs, idempotency.rs, signing_keys.rs, intents.rs}, crons/{mod.rs, intents.rs}, objects/stubs.rs, quota/mod.rs}`,
`crates/core/src/{keys.rs, crypto.rs}` (key format and the sealing envelope, pure)
(`TenantQuota` as a stub class that answers every `QuotaRequest` variant, see below),
`deploy/wrangler.toml.tmpl` whole and the `export_worker!` call with the seven classes, six of them stubs
([Shared files](#dependency-graph)); `ratelimit.rs` holds the compiled-in limit of every `RL_*` binding;
`log.rs` and `metrics.rs` are minimal writers (the typed log line with no free-text field, and the
`event = "metric"` line plus the Analytics Engine data point) that M17 Foundation extends with the
catalogued labels and the alert evaluator, so M5 can already log and count; `crons/intents.rs` re-sends
the `rpc_intents` rows ([Design conventions § 9](design/index.md#9-durable-object-calls-after-a-d1-change)),
`migrations/d1/0001_init.sql` (every D1 table and index in [Data model](design/data-model.md), including those that
later milestones use: the console, billing, sign-up and domain-method tables and columns),
`crates/worker/tests/` harness (`cargo xtask itest`).

**Implements:** FR-TEN-1/2/3, FR-KEY-1/2/3/4/5, NFR-SEC-1 (the cross-tenant suite), the error envelope, rate limits, request IDs,
idempotency for non-mail POSTs, and the thread and link keyring (`signing_keys`, created on first use;
[Security](design/security.md#62-rotation-procedures)).

**Partners and partner keys** (FR-KEY-4) land here with the other key levels: the `partners` table and
the `partner_id` columns of `tenants`, `api_keys` and `webhook_endpoints`, `tenants.suspended_by` and
`policy_ceilings_json`, `partners.max_tenants` and `ramp_exempt`, `zone_claims` and
`event_index.partner_id` (all in `0001_init.sql`); the five `/v1/partners` routes, with the soft delete;
`level: "partner"` in `POST /v1/keys`; the partner checks of authentication (`403 partner_suspended` for
the partner's keys and for its tenants' keys) and of the owner check, which compares only a partner key's
own `partner_id` with a tenant's, so a `NULL` never matches
([Security › Partner keys](design/security.md#partner-keys)); `POST /v1/tenants` with a partner key (its
`partner_id`, `default_billing_mode`, `max_tenants` and `RL_PARTNER`, `RlBucket::Partner`); the
per-field policy classes and platform ceilings of
[Configuration › Who may change a field](../reference/configuration.md#who-may-change-a-field);
`suspended_by`; the frozen `erasing` and `erased` tenant for non-platform writes; idempotency records
keyed by the calling key, with one-time secrets stripped; and the `foreign_partner` class of the
cross-tenant suite. Partner endpoints (`POST /v1/webhooks` with a partner key, and their scoped delivery)
land with the webhook routes in M8, and `quarantine.key_release` with the release route in M7.

**Workspace policy** (FR-TEN-4, [Workspace policy](design/workspace-policy.md)) lands here with the
policy classes: the `policy:write` permission (tenant-only, refused on identity keys),
`GET` and `PATCH /v1/tenants/{tenant_id}/policy`, the one `policy::write` service that every policy write
goes through (`POST /v1/tenants`, `PATCH /v1/tenants/{tenant_id}` and the new route) with
`core::policy::check_write` and the `FIELDS` table, the guard class, the partner ceilings
(`tenants.partner_ceilings_json`), the compare-and-set on `tenants.policy_version`, the `tenant.policy_update`
audit row and the `tenant.policy_updated` platform event row, and `auth::people::key_may_decide`, the rule
for decisions reserved for people that M7's release and M27's approval also call. The policy field
`accounts.require_approval` is stored and classed here; it takes effect with M27. The console's policy page
comes with M21.

**The cross-tenant matrix grows with each route family.** J10's matrix (`it::security::cross_tenant_matrix`
with its `foreign_partner` class, and `it::partners::j10_foreign_partner_not_found`) starts here with the
tenant, partner and key routes. Each milestone that adds a route family extends both in the same pull
request: M6 the identity and address routes, M7 the message, thread and quarantine routes, M8 the webhook
endpoint routes, and M13 the domain routes, with M13 also adding the `foreign_zone` case
([H8](edge-cases.md)).

**Keys** ([Security §4.5–§4.6](design/security.md#45-rotation)): the reach and not-wider checks on reading,
rotating and revoking a key (J20); the `identities:sign` grant by platform and partner keys; the
`tenants:erase` permission and the owner-only rule on tenant keys (`created_by_role`; the console session
that writes `owner` lands in M21, so M5 tests the rule with a key row seeded as an owner's); the
`created_by_user_id` and `created_by_role` columns, copied from the calling key on every mint (the
revocation on member removal and role change lands with the member routes in M21); the active-key caps
and the `RL_TENANT` and `RL_PARTNER_API` buckets (J22); `404 route_not_found` and
`405 method_not_allowed` in the router.

**Tenants without owner or billing behaviour.** `POST /v1/tenants` accepts `owner` and `billing` as
[REST API](../reference/api.md) specifies, validates them, and stores them: the owner's `users` and
`members` rows and the `billing_accounts` row. Nothing acts on them yet. The owner's sign-in link is sent
once M21 lands, and plan checks run once M22 lands; until then holds always succeed, as in billing mode
`disabled`. Each tenant gets its `TenantQuota` object (`quota_do_id`, an `rpc_intents` row in the tenant's
D1 batch, then `QuotaRequest::Init`; a lost `Init` is re-sent by `crons/intents.rs`, J21).
`notify_do_id` is written as `''` until M26 mints a `Notifier` with the tenant row; from M26 the
every-minute cron also mints one for each row still at `''`
([Data model](design/data-model.md#1-d1-control-plane)).

**The `TenantQuota` stub.** `quota/mod.rs` declares the whole `QuotaRequest` enum of
[Outbound › TenantQuota](design/outbound.md#tenantquota) and
[Plans, metering and billing](design/billing.md#tenantquota-allowances-and-holds) now, and the stub
answers every variant, so a milestone that calls `TenantQuota` before the one that owns a variant gets a
well-formed answer. Later milestones replace behaviour, never a signature:

| Variants | Stub behaviour from M5 | Replaced by |
|---|---|---|
| `Init` | Stores the owner in `meta`; every other request checks it | final |
| `Reserve`, `Release` | `Reserve` takes its `sends` hold as `Hold` does and counts the day's `sends:{identity_id}` counter, and the tenant `sends` counter unless `tenant_cap` is `None` (the system identity), without enforcing a cap; `Release` decrements the same counters (`sends` only when `tenant_counted`) | M9: daily caps (`CapReached`) and `quota.warning` thresholds |
| `RecordOutcome` | Records the outcome in `outcomes` and the tenant's per-day outcome counters (once per `event_id`); answers `OutcomeDecision` with every field false or `None`, so it never pauses an identity, a tenant or a domain | M9: abuse auto-pause (FR-DLV-3) and the tenant and domain pause (FR-DLV-6, G12) |
| `AdmitInbound` | Counts `inbound_hour`; always `Admit` | M7: the per-tenant inbound cap (`Throttle`, D13) |
| `CountTriageModel` | Counts `triage_model` once per `ref`; always `Ok` | M12: the daily model cap (`CapReached`) |
| `OutcomeRates` | Zero counts (`{ outcomes: 0, bounced: 0, complained: 0 }`) | M24: sums the tenant's outcome counters for the send ramp |
| `SystemMail` | Always `SystemMailAnswer::Ok`; counts nothing | M24: the system-mail budgets (`sysmail:{class}:{key}` counters, `Spent { scope }`) |
| `CountAgentic` | Counts `agentic` for the day and `usage:agentic`; always `Ok { used }` | M11: the tenant daily cap (`CapReached`) |
| `RecordUsage` | Adds to `usage:{metric}` for the current UTC day | final |
| `ForgetIdentity` | Deletes the identity's `outcomes` rows and `sends:{identity_id}` counters | final |
| `Hold` | Always grants (`HoldAnswer::Held`, `remaining: None`) and counts the units in `allowances.held`, as in billing mode `disabled` | M22: allowances and holds |
| `Settle`, `Adjust` | `Settle` moves `consume` units of the hold to `allowances.used` and releases the rest; `Adjust` adds `delta` to `used` (never below 0). No limit is checked | M22 |
| `Extend`, `SetPlan`, `SetMeasured`, `Reconcile` | No-ops that answer `()` | M22 |
| `GetUsage` | `UsageAnswer` with mode `Disabled`, no period, and one row per feature with `granted: None` and the `used` and `held` counts its holds keep | M22 |

**Acceptance:**

- Authentication ([Security § 13](design/security.md#13-tests)): `it::auth::unknown_key_uniform`,
  `it::auth::status_after_secret_match`, `it::auth::rotation_overlap`, `it::auth::last_used_throttle`
  and `it::auth::missing_permission`.
- Keys: `it::keys::scope_exceeded` (creation and the management of existing keys, J20),
  `it::keys::permission_level_rules` (its `GET /v1/usage` part is added in M22, with that route),
  `it::keys::j6_revoke_rotate` with `GET /v1/audit-events?actor_key_id=` (J6), and J22
  (`it::auth::j22_tenant_aggregate_limits`). The owner-only part of W35 (`tenants:erase` refused on a
  tenant key minted by any caller but an owner's key) runs here with a seeded owner's key; W35 is
  accepted in M21.
- Routing and isolation: `it::routes::unknown_route_and_method`, `it::security::route_table_complete`,
  `it::security::body_scope_ignored`, `it::security::rpc_owner_mismatch`,
  `it::security::response_headers`, `worker::db::no_formatted_sql` and
  `xtask::ratelimit_limits_match_template`. A cross-tenant suite skeleton: for every registered route, a
  key from another tenant gets an indistinguishable `404` (or the same `403` as its control call). The
  suite enumerates the router table, so a new route without a test fails.
- The keyring creates one key per purpose under concurrency and opens it with `PM_MASTER_KEY`
  (`it::keyring::one_key_per_purpose_concurrent`, `core::keys::format_round_trip`,
  `core::crypto::envelope_round_trip`).
- Tenants: `it::tenants::create_idempotent_replay` (replays and conflicts; `owner` and `billing` are
  stored but send no mail and change no limit); the tenant's `TenantQuota` answers `Init`, refuses a
  mismatched owner, and answers every other variant as the stub table says
  (`worker::quota::stub_answers_every_variant`, a worker-logic test that sends each one); the
  `TenantQuota` part of J21 (`it::intents::j21_init_retried` for the tenant: a lost `Init` is re-sent
  by the cron; its mailbox part lands in M6, which accepts J21).
- `/health`, `/v1/me`, `/openapi.json` and `/.well-known/security.txt` are served (the last as
  [Security](design/security.md) specifies, from `PM_SECURITY_CONTACT`:
  `it::security::well_known_security_txt`), with the build-time version, commit and date.
- Partners (FR-KEY-4): `it::partners::routes_and_audit` (its `PATCH /v1/tenants/{own}/billing` case is
  added in M22, which builds that route); `it::partners::policy_caps_lower_only` (its
  `send_policy.daily_cap` cases are added in M6 with the identity routes); J10
  (`it::partners::j10_foreign_partner_not_found`, and the `foreign_partner` class of
  `it::security::cross_tenant_matrix`, for this milestone's routes); J11
  (`it::keys::j11_partner_key_limits`); a partner key's requests counted in `RL_API` by key ID
  (`it::auth::rate_limited`, whose `RL_SIGN` case is added in M25 with the signing routes).
  Parts of four rows run here and the rows are accepted later, as for J13: J18's `max_tenants` race and
  its `RL_PARTNER` count of tenant creations run here (`it::partners::j18_partner_limits`); its
  erased-tenant case runs once M14 can erase a tenant and its invitation case once M21 builds
  invitations, and J18 is accepted in M21. The authentication part of J13 runs here
  (`it::partners::j13_suspended_partner`: partner and tenant keys refused with `403 partner_suspended`);
  J13 is accepted in M9, once inbound storage (M7), held deliveries (M8) and the send path (M9) exist. The
  `suspended_by` and ceiling parts of J17 run here too, the partner's no-op suspension of a
  platform-suspended tenant included; J17 is accepted in M9 with its abuse-pause part. The key part of J19
  (`POST /v1/keys` and key rotation) runs here; J19 is accepted in M8 with the webhook secrets.
  `DELETE /v1/partners/{partner_id}` ships here with its `409 partner_has_tenants` and soft delete; J12
  is accepted in M14, because its test erases the partner's tenant first.
- NFR-SEC-1: the cross-tenant suite (`it::security::cross_tenant_matrix`), with its `foreign_partner`
  class, finds 0 cross-tenant reads or writes. Every later milestone extends it with its routes, and it
  must stay at 0.
- Workspace policy (FR-TEN-4): `core::policy::workspace_write_table`; J23 (`it::policy::j23_workspace_ceilings`),
  J24 (`it::policy::j24_workspace_field_classes`), J25 (`it::policy::j25_guard_fields_need_person`), J26
  (`it::policy::j26_partner_ceiling`), J27 (`it::policy::j27_concurrent_writes`) and
  `it::policy::policy_updated_event_and_audit` (the `event_index` row here; its delivery to the tenant's,
  partner's and platform endpoints is asserted once M8 lands); the two `/policy` routes join the
  cross-tenant matrix.

**One migration until v1.0.** `0001_init.sql` holds every table until v1.0 is released. No later
milestone adds a D1 migration: M6–M26 change code only. A milestone that finds a missing column or table
fixes `0001_init.sql` itself (no deployed database exists before M20) and updates
[Data model](design/data-model.md) in the same pull request. Migrations `0002_…` onwards start after v1.0,
under the expand-then-contract rule ([CLI and setup §8.5](design/cli.md#85-d1-migrations)).

**Owner of shared files from here:** Track 1 owns `router.rs` and `0001_init.sql`. Other tracks add routes
through `handlers/<area>.rs` plus one registration line, reviewed by Track 1.

---

## M6 · Identities, addresses, platform domain (Track 1)

**Files:** `handlers/{identities.rs, addresses.rs (the list, `GET /v1/identities/{identity_id}/addresses`;
there is no get-by-ID route, and an address is also read in its Identity object; the platform address is
created with the identity), domains.rs (platform domain read only)}`, `db/{identities.rs, addresses.rs, domains.rs}`,
`mailbox/mod.rs` (the `IdentityMailbox` class, replacing M5's stub, with schema-on-wake),
`mailbox/schema/v1.sql` (every mailbox table of [Data model §2](design/data-model.md#2-identitymailbox-durable-object-sqlite),
all of them now, each statement idempotent with `IF NOT EXISTS` as
[Design conventions §4](design/index.md#4-durable-object-transactions) rule 6 requires; later milestones
fill the tables and never add one before v1.0), `mailbox/outbox.rs` (the transactional
outbox, its dispatch alarm, the `event_index` writes and the `pm-webhooks` producer;
[Webhooks and events](design/webhooks.md#transactional-outbox)), `webhooks/envelope.rs` (the event
envelope builder, the `WebhookJob` queue message and the `identity_*` and `address_*` payload builders,
which the outbox needs before M8 exists; M8 imports it), `jobs/mod.rs` as a `JobRunner` stub (below), and
the system identity's mailbox minting in the every-minute cron. Hooks in M5's files, which Track 1 owns
too: the mailbox `init` and `emit_event` intents in `handlers/identities.rs`, and in
`handlers/tenants.rs` the `identity.paused` and `identity.resumed` intents of a tenant suspension
([Design conventions § 9](design/index.md#9-durable-object-calls-after-a-d1-change)); the `inboxes` hold
in identity create and the `Adjust −1` in identity delete, against M5's `TenantQuota` stub, which grants
every hold until M22 ([Identities and domains › Create](design/identity-domains.md#create), step 6).

**Implements:** FR-IDN-1–4, FR-ADR-5–7, FR-DOM-1 (platform), the outbox and event index, and the system
identity ([Identities and domains](design/identity-domains.md#the-system-identity)). FR-ADR-1–4 (several
addresses, promotion, retirement and rollback) need a tenant domain and land in M13.

**Identity deletion before M14.** `DELETE /v1/identities/{identity_id}` runs its whole D1 batch
([Identities and domains › Delete](design/identity-domains.md#delete-fr-idn-4-a13)): tombstones, address
removal, `status = 'deleting'`, and the `jobs` and `erasure_requests` rows. Until M14 the `JobRunner` is a
stub that accepts `JobRequest::Start` and runs no step, so the job stays `queued` and the identity
`deleting`. M14 replaces the stub with the real `JobRunner`, whose every-minute restart of jobs left
`queued` picks these up.

**Acceptance:** A5, A12, A18 (`it::identities::a18_default_tenant_platform_only`), A17 (`it::identities::a17_system_protected`; its erasure and suspension cases run from
M14 and M21), J9 (`it::mailbox::j9_migration_on_wake`, including a migration rerun after a
lost `schema_version` write), J21 (`it::intents::j21_init_retried`, its mailbox part, and
`it::intents::j21_identity_event_retried`), `it::identities::deleting_and_deleted_routes`, the
`inboxes` hold taken and settled on create and adjusted on delete (asserted on the stub's counters), the
identity and address routes added to J10's matrix
(`it::security::cross_tenant_matrix`, `it::partners::j10_foreign_partner_not_found`), the
`send_policy.daily_cap` cases of `it::partners::policy_caps_lower_only`, plus `identity.created`, `identity.updated`,
`identity.paused` and `identity.resumed` events that reach the outbox, `event_index` and a `pm-webhooks`
message (consumed once M8 lands); a crash between commit and dispatch repeats the dispatch, never loses
it. The system identity is never listed and refuses tenant keys. The pause itself
(`PATCH /v1/identities/{identity_id}` with `status: "paused"` or `"active"`) ships here. Rows that need a
later milestone are accepted there: A7 in M9 (its only test, `it::send::a7_paused_refuses_send`, needs
the send path), A13 in M7 (it needs `email()`), A8 in M9 (`it::send::a8_owner_required`), and A11, A14
and the promote, retire and rollback flows in M13 (they need a tenant domain).

---

## M7 · Inbound (Track 1)

**Files:** `email.rs` (handler), `consumers/inbound.rs`, `mailbox/{ingest.rs, threads.rs, messages.rs, attachments.rs, read.rs}` (on M6's `schema/v1.sql`),
`handlers/{threads.rs, messages.rs, quarantine.rs, wait.rs}`, `consumers/index.rs` (attachment text only at
this stage; the other job kinds ack without work, [Search § 6](design/search.md#6-indexing-pipeline-pm-index)),
`crates/api-types/src/internal/index_job.rs` (the whole `IndexJob` enum, so M10 and M12 only
fill in their arms), `quota/mod.rs` (`AdmitInbound`: the per-tenant inbound cap), and the suffix fold
rule of D12 in M5's `handlers/tenants.rs`.

**`email.rs` owns every special branch from M7**, in the order of
[Inbound › Steps](design/inbound.md#steps), so later milestones fill in a branch and never reorder the
handler. Until its owner lands, each branch is a stub: `journal` returns `Ok` and stores nothing (M9 adds
strategy B's Message-ID learning); the forwarding check treats every token as unknown and `pm-probe`
returns `Ok` with no effect (M23 creates the tokens); the RFC 2142 operational names and tenant-domain role
addresses answer `550 5.1.1` (M13 builds `inbound/role_mail.rs`, the relay through the system identity).
No deployment runs these stubs in production; the first staging deploy is M20.

**Implements:** FR-IN-1–12, FR-THR-1/2, NFR-REL-1/2, read APIs, quarantine and release (with the
`quarantine.key_release` override of FR-CON-6 for API keys, through M5's `auth::people::key_may_decide`), and `wait`
([Inbound › The `wait` handler](design/inbound.md#the-wait-handler-e4)), which is P0 because quarantine
rule 5 (E5) depends on its registrations.

**Acceptance:** A2, A6 (`it::inbound::a6_reject_codes`; its SES part in M23), A9, A10 (inbound part), A13, A16, B1 (documented), B2 (with `it::inbound::panic_to_degraded`), B3, B12 (`it::index::b12_extraction_failure`; the search part in M10), B14, C1 (with `it::thread::seq_never_reused`), C9, C10 (`it::inbound::c10_forged_participant_unverified`), C11 (`it::inbound::c11_token_flood_keeps_replies`), D4, D5, D9, D10,
D12, D13 (`it::inbound::d13_unsolicited_flood_caps`; the triage cap in M12),
E4 (`it::wait::e4_*`), E5, J1, J2, J7, J14 (`it::quarantine::j14_key_release_policy`), J16
(`it::quarantine::j16_key_release_override`), the message, thread and quarantine routes added to J10's
matrix, and every `conf::` corpus case ingested end to end through workerd. The read path, labels and
release (FR-IN-10): `it::messages::read_path_visible_threads`, `it::messages::labels_and_read_state`,
`it::messages::thread_counts_visible_only` and `it::quarantine::release_transaction`; the job kinds that
later milestones fill in: `it::index::job_kinds_before_milestone`.
Rows whose inbound side needs a later milestone are accepted there: C7 (it matches replies to outbound
mail), D7 (suppressions and lists) and loopback L3 in M9, C3 (a retiring address), A4's role-mail
routing and D14 (they send a new message and need a tenant domain) in M13, D11 (a DSN is trusted only for
an SMTP-relay send) in M23, and A15 and I10 (they need erasure and exports) in M14.
NFR-REL-1 is checked with a canary, because the only emitter of `inbound_lost_total` is the global
retention `staging` step, which comes with M14: under the J1, J2 and J7 fault injections, every message
that `email()` accepted is found exactly once through the read API after the queues drain, and
`inbound_raw_missing_total` stays 0 (`it::inbound::nfr_rel1_no_loss_canary`). NFR-REL-2: the inbound SLI
counters of [Observability §4](design/observability.md#4-service-level-objectives) are emitted for accepted, staged and
temporarily failed mail.

---

## M8 · Webhooks (Track 2, after M6)

**Files:** `handlers/webhooks.rs`, `consumers/webhooks.rs`, `webhooks/{sign.rs, client.rs, replay.rs, payloads.rs}`
(it imports the envelope, `WebhookJob` and the identity payload builders from M6's `webhooks/envelope.rs`),
`crons/outbox_sweep.rs`, the SSRF guard `crates/core/src/ssrf.rs` (pure) and `crates/worker/src/net.rs` (guarded HTTP)
([Webhooks](design/webhooks.md), [Security § 9](design/security.md#9-ssrf-controls)).

**Implements:** FR-WH-1–5, NFR-REL-4, and the partner endpoints of FR-KEY-4 (`scope: "partner"`).

**Acceptance:**

- Signature vectors from the Standard Webhooks spec verify.
- Rotation sends two signatures.
- The SSRF table refuses loopback, RFC 1918, link-local, CGNAT, `::1`, `fc00::/7` and
  `169.254.169.254`, and does not follow redirects (`core::ssrf::refuses_private_ranges`).
- J4: a time-controlled harness checks the retry schedule.
- Replay.
- Auto-disable on `410` and on 100 consecutive failures.
- J15: partner endpoints receive only their partner's tenants' events, by fan-out and by replay, and
  `webhook.disabled` for a partner endpoint reaches that partner's other endpoints and platform endpoints
  (`it::webhooks::j15_partner_scope_filter`), with replay selecting on `event_index.partner_id` and never
  replaying `webhook.test` (`it::webhooks::replay_by_ids_and_window`). M8 also adds the endpoint routes to
  J10's matrix (another partner's endpoint by ID, in `it::partners::j10_foreign_partner_not_found` and
  `it::security::cross_tenant_matrix`).
- J29 (`it::webhooks::j29_mail_events_need_messages_read`): endpoints that receive mail events need
  `messages:read` (and `quarantine:review` for `message.quarantined`) to create, change or replay, and a
  changed URL waits for a passing test.
- The system identity's events reach platform endpoints only, by fan-out and replay
  (`it::webhooks::system_identity_events_platform_only`, part of A15), and an event whose identity was
  deleted is recorded `dead` with `event_unavailable` at once (`it::webhooks::deleted_identity_event_unavailable`,
  with the mailbox's `erased` flag set through the itest hooks).
- J19 (`it::idempotency::j19_per_key_no_secret`): replays are per key, and a replay of key creation and
  rotation, webhook creation and webhook secret rotation returns no secret (`"secret_replayed": false`).
- The delivery hold of J13 (`it::webhooks::j13_held_while_partner_suspended`): deliveries to a suspended
  partner's endpoints and its tenants' endpoints are parked in `webhook_held` and released by the
  every-minute cron on reactivation.
- NFR-REL-4: the retry schedule reaches 24 hours within its 13 attempts, and `webhook_delivery_latency_ms`
  and `webhook_dead_total` are emitted for the SLI.
- `webhook_endpoints.secret_enc` and `prev_secret_enc` are registered in `crates/core/src/sealed.rs`
  (M17 Foundation's registry), with their case in `it::secrets::master_key_rotation`.

---

## M9 · Outbound and delivery (Track 1, after M7)

**Files:** `handlers/send.rs`, `mailbox/{submit.rs, compose.rs, locks.rs, deliveries.rs, idempotency.rs}`,
`crates/core/src/compose.rs` (MIME composition, pure),
`transport/{mod.rs, cloudflare.rs, simulator.rs, loopback.rs, breaker.rs}`, `consumers/{outbound.rs, delivery.rs}`,
`domains/cloudflare_api.rs` (created here with only the Email Sending suppression lookup of G4; M13 owns
the file from then on and adds the zone, routing and sending calls),
`quota/mod.rs` (replaces the M5 stub's `Reserve`, `Release` and `RecordOutcome` behaviour with the daily
caps, quota warnings and abuse windows; `RecordUsage` has recorded since M5),
`handlers/{suppressions.rs, lists.rs, links.rs}` (allow and block lists,
`/v1/tenants/{tenant_id}/lists/{direction}/{kind}[/{entry}]`; signed links, `GET /v1/links/{token}`, with
kid verification), `db/{suppressions.rs, lists.rs}`, `mailbox/alarms.rs` (the claim and dispatch purposes
of [Data model §2](design/data-model.md#2-identitymailbox-durable-object-sqlite): thread locks expire
lazily and reconciliation is event-driven, so neither has an alarm; no cron is involved).

**Implements:** FR-OUT-1–16 (FR-OUT-15's re-check at transport in M13), FR-DLV-1–7, NFR-PERF-1/2.

**Acceptance:** A7, A8, A10, C2, C4, C6, C7, D6 (exchange cap), D7, E2, E3 (with
`it::send::e3_timezone_change_caps`), E8, G1–G6 (G2 with `it::send::stale_claim_outcome` and
`it::send::g2_delivery_failed_recipients`; G4 with `it::send::g4_suppressed_after_submit`), G8–G11 (G5 with
signed links; G9's re-check of the marketing transport at `BeginTransport`, which needs an `ses` domain, is
accepted in M13), G12 (`it::delivery::g12_tenant_domain_pause` and `it::send::sending_paused_refuses`; the
domain part seeds a tenant domain row through the itest hooks), G13
(`it::delivery::g13_first_reply_outcomes_not_counted`), I9 (`it::erasure::orphan_objects_swept`,
with the mailbox's daily sweep in `mailbox/alarms.rs`), J28 (`it::send::j28_transport_breaker`), the loop
guard of N13 (`it::send::n13_loop_guard`; the forwarding part in M23) and the reply rate of N35
(`it::send::n35_reply_rate`), K3, L1–L4. J13
(`it::partners::j13_suspended_partner`, whose authentication part runs from M5): no send is accepted for
a suspended partner's tenants, their inbound mail is stored, and with M8's held deliveries the row is
accepted here. J17 (`it::partners::j17_operator_enforcement`): with the abuse auto-pause, resuming an
identity paused for `abuse_threshold` on a partner's tenant needs a platform key. G3's `it::ops::provider_quota_80` fires through M17 Foundation's evaluator,
which merges first. G7 needs domain states and is accepted in M13. Also: the allow and block lists
(`it::lists::entries_crud`) and their effect on sends and on inbound mail (`it::send::list_filters`,
`it::inbound::receive_allow_skips_spam`; D7 covers receive-block), the custom-header rules checked at the
API (`it::send::header_rules`), the simulator matrix drives every status, an uncertain send is reconciled by a
later provider event, `?dry_run=true` returns the recipient plan without storing anything, and cancel
works only while a message is `queued` and unclaimed (`it::send::cancel_queued`, FR-OUT-11).
NFR-PERF-1 (`it::bench::send_api_p95`) and NFR-PERF-2 (`it::bench::queue_to_transport_p95`) report their
figures; CI warns above the targets.

---

## M10 · Search (after M7)

**Files:** `search/{mod.rs, keyword.rs, semantic.rs, hybrid.rs, rerank.rs, facets.rs, cursor.rs, tenant.rs, contacts.rs, related.rs}`,
`mailbox/search.rs`, `consumers/index.rs` (chunk, embed, upsert), `handlers/{search.rs, contacts.rs}`, `crons/index_reconcile.rs`.

**Implements:** FR-SRCH-1–7, 10 and 11 (index side), FR-SRCH-12 (contacts and related); NFR-PERF-3/4/5.

**Acceptance:** F2 (`it::auth::f2_permission`, now that search exists), F3–F5, F7, F8, F14, F15, F16
(`it::index::f16_ai_fair_queue`; the triage part in M12), the search part of B12
(`it::search::b12_unavailable_in_why`), and the tenant fan-out's exclusion of the system identity (part of
A15) (F6 needs erasure and is accepted in M14). The nightly reconciliation records its run in D1 `index_reconcile` and
raises the drift alert only after two nights over 1% ([Search § 6.6](design/search.md#66-nightly-reconciliation)).
NFR-PERF-3: keyword p95 ≤ 200 ms on a 50,000-message synthetic mailbox in workerd (`it::bench::keyword_p95`,
which reports the figure, with a warning threshold). NFR-PERF-4 (`it::bench::hybrid_p95`) and NFR-PERF-5
(`it::bench::tenant_fanout_p95`) report theirs the same way. The benchmarks fill their mailboxes through
the `itest-hooks` bulk-seed hook and run in the nightly workflow, not as a required check on every pull
request ([Testing § 6.9](design/testing.md#69-benchmarks)).

---

## M11 · Agentic search (after M10)

**Files:** `search/agentic/{mod.rs, planner.rs, tools.rs, judge.rs, answer.rs, sse.rs, prompts.rs}`,
`quota/mod.rs` (`QuotaRequest::CountAgentic`: the tenant daily cap replaces the M5 stub's plain count).

**Implements:** FR-SRCH-8/9, FR-SRCH-13, NFR-PERF-6.

**Acceptance:**

- E1 (fenced, and `core::injection::e1_fact_refs_normalised` for the `why=` line), F10–F13, F17
  (`core::citations::f17_untrusted_sources_removed`, `it::agentic::f17_answer_untrusted`), F18
  (`core::citations::f18_refs_from_authenticated_sources`).
- The `contacts` tool at tenant scope (`it::agentic::contacts_tenant_merge`), and one count per validated
  agentic request, with partner keys on the tenant route (`it::search::agentic_counted_once`).
- Deterministic tests with a scripted fake model: plan, two searches, refine, answer, then a
  verifier removal.
- An SSE stream test.
- Budget enforcement by steps and by time, and the tenant daily cap through `QuotaRequest::CountAgentic`.
- NFR-PERF-6: `it::bench::agentic_p95` reports p95 and first-evidence time against 8 s and 1.5 s.

---

## M12 · Triage (after M7)

**Files:** `triage/{mod.rs, rules.rs, model.rs, schema.rs, prompts.rs}`, `consumers/index.rs` (triage job),
`quota/mod.rs` (`CountTriageModel`: the daily model cap).

**Implements:** FR-TRI-1–5.

The `triage` hold of consumer step 2 ([Triage § 1.1](design/triage.md#11-consumer-steps)) goes to the M5
`TenantQuota` stub, which grants every hold, so triage runs on every message until M22 adds allowances.

**Acceptance:**

- D8 and rule evaluation order; E1 for triage input (`it::triage::e1_fenced`,
  `core::injection::e1_fact_refs_normalised` for the `FACTS` block).
- The triage part of D13 (`it::triage::daily_model_cap`) and of F16 (`it::triage::f16_rl_ai_deferral`).
- Re-run eligibility and the `run` counter (`it::triage::rerun`, FR-TRI-5), and the skipped record written
  at ingest (`it::triage::skipped_record_at_ingest`).
- Invalid model output ends `failed` and is never guessed.
- `message.triaged` events.
- The thread roll-up.

---

## M13 · Domains (after M7 and M9; SES depends on S8)

**Files:** `domains/{mod.rs, cloudflare_api.rs, ses_api.rs, ses_control.rs (SesControl DO, the SES
token bucket), records.rs, monitor.rs (DomainMonitor DO), fallback.rs, provider_objects.rs (the onboarding
journal and `provider_objects_json`)}` (the two classes replace M5's stubs; their bindings exist since M5), `crons/domain_cleanup.rs` (the hourly cleanup of failed adds),
`handlers/domains.rs` (full for `cloudflare_zone`, including `PATCH /v1/domains/{domain_id}` for the
transport), `handlers/addresses.rs` (aliases on tenant domains, promote, retire and rollback),
`crons/retire.rs`, `transport/ses.rs`, `consumers/ses_events.rs` (the `POST /hooks/ses` SNS endpoint), and
CLI `idvy domains subscribe`. The other connection methods, `nameservers` included, are M23's.

**Implements:** FR-DOM-2–6, FR-DOM-13 and FR-DOM-14 for `cloudflare_zone`, FR-ADR-1–4.

**Acceptance:**

- H1, H2 (`core::dns::h2_spf_lookup_count`; its MAIL FROM part, `it::ses::h2_mail_from_spf_preflight`,
  is for `dns_records` and `send_only` domains and is accepted in M23), H3–H7, H8
  (`it::domains::h8_zone_permission` for `cloudflare_zone`, `replace_mx` and the listed zones of
  `domains.cloudflare_zones`; the `zone_claims` written by `nameservers` and `delegated_subdomain`, and the
  refusal of those methods under a claimed or deployment zone, are added in M23), H18
  (`it::domains::h18_exact_name_grant`: exact-name entries and the MX check at any name), the domain routes added
  to J10's matrix with the `foreign_zone` case of `it::security::cross_tenant_matrix`, G7, C3, A11, A14, A4's
  role-mail routing (`it::inbound::a4_role_mail_routing`) and D14, its relay through the system identity
  (`it::inbound::d14_role_mail_relay_guarded`, with `inbound/role_mail.rs`), G9's re-check of the
  marketing transport at `BeginTransport` (`it::send::g9_marketing_transport_recheck`), the promote,
  retire and rollback flows
  (`it::addresses::promote_retire_rollback`, `it::addresses::retirement_cron`), and the API part of J5
  (`it::domains::transport_patch`, including the SES identity that `cloudflare_zone` onboarding creates
  for the failover when SES is configured), and the per-domain limits on verify and probe
  (`it::domains::verify_rate_limited`). J5's live part, `live::transport::j5_ses_failover`, runs in
  M20.
- The spike S9 fallback path for `cloudflare_zone`, whatever S9's result
  (`it::domains::s9_manual_delivery_events`, `cli::domains::subscribe_manual`), with its sends from the
  platform address until the subscription exists (H17, `it::domains::h17_manual_events_fallback`), and the
  SES control-plane budget (`it::ses::control_plane_rate`).
- H7's two-resolver failure (`it::domains::h7_dns_unresolvable`), the platform domain's health (H9,
  `it::domains::h9_platform_domain_failing`, with the `platform_domain_failing` page), no adoption for
  `cloudflare_zone` and SES identities (H10, `it::domains::h10_no_adoption`: existing sending domain,
  routing, catch-all and foreign-tagged SES identity; the zone parts are added in M23), the mail-domain
  limit per zone (H15, `it::domains::h15_zone_domain_limit`), the onboarding journal and
  `provider_objects_json` written by every create step, the uniform `409 domain_exists`, the claim record,
  the unverified cap and expiry (the add paths of H14; eviction and expiry finish through removal in M14),
  and the alerts `domain_remove_failed` and `delivery_events_manual` through M17 Foundation's evaluator.
- With a DNS fake that can remove a record, add a conflicting record, or move the NS, and the
  `cloudflare_zone` method only: `it::domains::{h1_failing_fallback, h4_ownership_change, h5_existing_mx,
  h6_rule_failure, onboarding_idempotent, records_from_api, cron_mints_missing_monitor,
  cf_token_required_by_method}` and `it::send::g7_domain_states`. The `nameservers` and
  `delegated_subdomain` parts of the onboarding, token and S9 fallback tests are separate tests, accepted
  in M23.
- The fallback send carries `sent_via_fallback` and keeps the thread token.
- Recovery leaves fallback threads pinned.
- The `domain_remove` job's steps for `cloudflare_zone`
  ([Identities and domains › Domain removal](design/identity-domains.md#domain-removal)), including
  `delete_ses_identity`, which deletes the failover SES identity that onboarding created and its three
  DKIM CNAMEs. Every step acts on the recorded provider IDs only, `disable_routing` removes one name's
  routing unless it is the zone's last routing domain at its apex, and absence is read back rather than
  inferred from a `404` code. `DELETE /v1/domains/{domain_id}` queues the job behind M6's `JobRunner` stub;
  M14 runs it, inline in tenant erasure's `remove_domains` too, and accepts
  `it::domains::remove_deletes_ses_identity` and the other removal tests.

---

## M23 · Domains on any DNS host (after M13 and M9; S11, S12 and S10 gate methods)

**Files:** `crates/core/src/{connect.rs, smtp.rs, sns.rs}` (and SES receipt parsing in `ses.rs`),
`handlers/domains.rs` (methods, `PATCH` with `smtp`, `probe`), `handlers/addresses.rs`
(`test-forwarding`), `handlers/hooks_ses.rs` (`POST /hooks/ses/inbound`), `transport/smtp.rs`,
`inbound/sources/{routing.rs, ses.rs}`, `consumers/inbound.rs` (the SES source), `crons/ses_backstop.rs`,
`domains/monitor.rs` (method health rows, probe and forwarding tokens), `domains/ses_control.rs` (the
retired-address rule sync, its single writer); no
migration (the `domains` method columns, `ses_ingest`, `addresses.ses_bounce_rule` and
`addresses.forwarding` are already in `0001_init.sql`); CLI `idvy setup ses`, `idvy domains add --method`, `idvy domains update
--smtp-…`, `idvy domains probe` and `idvy addresses test-forwarding`.

**Implements:** FR-DOM-7–12, [Domains on any DNS host](design/domain-connections.md).

**Acceptance:**

- N1–N30, with the tests named in the register (`core::sns::verify_v2_vectors`,
  `core::smtp::state_machine`, `core::dns::doubled_name_detected`,
  `it::ses::*`, `it::smtp::*`, `it::forwarding::*`, `cli::setup::ses_region_check` and
  `it::domains::{existing_mx_external, nameservers_dedicated_check, zone_expired, mx_wrong_region,
  zone_create_rate_limited, zone_hold, delegation_removed, ses_identity_limit}`).
- `core::connect::method_matrix`, which no register row names: every `method` maps to the documented
  `kind`, `inbound` and `transport`, and invalid combinations are refused.
- The `nameservers` and `delegated_subdomain` parts of M13's domain tests:
  `it::domains::onboarding_idempotent_created_zones` (with the monitor's failed and refused onboarding
  steps), `it::domains::s9_manual_delivery_events_nameservers`
  and `it::domains::cf_token_required_other_methods`, the zone part of H10 (an existing zone is never
  adopted; the pending claim; `it::domains::h10_no_adoption`), the DS check of H7, and the claimed-zone
  part of H8
  (`it::domains::h8_zone_permission`: `zone_claims` written with the domain row, deleted by `delete_zone`
  and on `zone_expired`, and the refusal of a name under a deployment or another tenant's zone).
- `idvy setup ses` is idempotent: it runs twice against a recorded AWS API fake with no duplicate
  resources, never deactivates an existing active rule set, and prints the IAM policy before applying it.
  It tags every taggable resource with the API host and refuses another deployment's resources (N31,
  `cli::setup::ses_foreign_resources`).
- The retired-address rules have one writer, `SesControl`, which verifies each write by reading back (N32,
  `it::ses::retired_rule_single_writer`); SES-sourced mail for a domain without `inbound = ses` is dropped
  (N33, `it::ses::non_ses_recipient_dropped`); a probe timeout after a pass is fail-level from the second
  (N34, `it::smtp::probe_timeout_after_pass`). The alert `ses_rule_sync_failed` fires through M17
  Foundation's evaluator.
- Every new error code and `transport_unavailable` reason in the design is returned by at least one test.
- The SES parts of rows that M7 and M13 accept: A6's suspended-tenant hold (`it::ses::suspended_tenant_held`)
  and H2's MAIL FROM preflight (`it::ses::h2_mail_from_spf_preflight`).
- D11, whose trusted path needs an SMTP-relay send (`it::inbound::d11_forged_dsn_ignored`).
- The SES operator alerts, which fire through M17 Foundation's evaluator: `it::ops::ses_alerts`
  (`ses_identities_90pct` for N26, `ses_sending_paused` and `ses_rule_missing` for N10).
- The cross-tenant suite covers `/hooks/ses/inbound` (no key) and the new routes.
- Erasure extension, once M14 has landed (if M14 lands later, it writes this substep instead of a stub):
  tenant erasure's `remove_domains` removes each SES domain's addresses from the `pm-retired-{n}`
  receipt rules (the `prune_retired_rules` step of domain removal;
  [Privacy §6.6](design/privacy.md#66-tenant-scope)). The SES identity itself is deleted by M13's
  `delete_ses_identity` step. `it::erasure::tenant_ses_rows` lands here.
- The global retention job's `ses_ingest` step (`it::retention::global_ses_ingest`).
- `domains.smtp_sealed` and `domains.smtp_pending_sealed` are registered in `crates/core/src/sealed.rs`,
  with their case in `it::secrets::master_key_rotation`.

**Gate:** each method ships only when its spike passed: S11 for `dns_records`, S12 for `smtp_relay`, S10
for `delegated_subdomain` (which also stays behind `PM_CF_SUBDOMAIN_SETUP`). `smtp_relay` with `inbound: ses`
also needs S11; without it, `smtp_relay` ships with `inbound: forward` only. `cloudflare_zone`,
`nameservers` and `send_only` do not wait for them. A method whose spike failed moves to v1.1 by ADR
(PRD section 5).

---

## M14 · Privacy (after M9, M10, M11, M12 and M13)

**Files:** `jobs/{mod.rs (JobRunner DO, replacing M6's stub), erasure.rs, retention.rs, export.rs, reembed.rs, reparse.rs, reindex.rs, backup.rs}`,
`handlers/{erasure.rs, exports.rs, holds.rs}`, `crons/retention.rs`.

**Implements:** FR-PRV-1–6, FR-IDN-4, NFR-PRV-1, and partner deletion after its tenants' erasure (FR-KEY-4).

**Acceptance:** F6, I1–I7, I11–I14 (`it::erasure::i11_inflight_copies`, `it::export::i14_export_permissions`,
`it::erasure::i12_provider_suppressions`, `it::jobs::i13_finalise_on_fail_and_cancel`), the tenant-scope hold behaviour of I2
(`it::erasure::i2_hold_tenant_scope`, with the global job's `held_erasures` step), the staging lifetime of
J7 (`it::inbound::j7_staging_outlives_dlq`, with the 15-day lifecycle rule and the daily re-queue of the
`staging` step), batched D1 deletes (`it::erasure::batched_deletes`), the `reparse` job that J3 starts (J3 itself is accepted with M17 Completion,
which adds its start through `POST /v1/platform/jobs`), plus every erasure scope with receipt counts and
empty probes (identity and tenant scope also delete `identity_keys` and write `key_tombstones`; with M25
this is O7), J12 (a partner whose tenants are all erased can be deleted, softly, and not before:
`it::partners::j12_delete_with_tenants`), I8 (writes to an `erasing` or `erased` tenant refused for
non-platform keys, reads kept for its partner key, a second tenant-scope erasure answered `200` or
`409 tenant_erased`, a failed tenant erasure resumed at its failed step by a new request, and the
tenant's idempotency records deleted by `tenant_id`: `it::erasure::i8_erasing_tenant_frozen`), a failed
erasure of any scope resumed by a new request for the same scope and target (`it::erasure::resume_failed_any_scope`), J18's erased-tenant case (an `erased` tenant frees its place
under `max_tenants`, in `it::partners::j18_partner_limits`), the optional backup copy (`it::retention::backup_copy`), and
`it::logs::i5_no_content_in_logs`, which greps captured Worker logs for any test-message body string and
any test address. I10 (`it::erasure::race_with_index_jobs`) and A15
(`it::security::a15_system_mailbox_unreachable`: the system identity's mail stays out of tenant search,
agentic tools, exports and tenant endpoints). NFR-PRV-1: in a time-controlled harness every erasure scope completes within 24 hours,
and a step that keeps failing is retried until 20 hours after the request, pages `erasure_stalled`
on its third failure and still produces a receipt (`it::erasure::step_retry_and_fail`); every job runs
`finalise` when it fails or is canceled; the `CounterpartyHash` target of `ErasurePlan` matches what
`Counterparty` matches (used by M17's re-applied erasures). Tenant scope
runs its steps in order, `cancel_billing` second (`it::erasure::tenant_scope_order`). `remove_domains`
runs M13's `domain_remove` steps inline, including `delete_ses_identity` (the failover SES identity and
its three DKIM CNAMEs), and a domain removal that M13 queued behind the stub now runs
(`it::domains::remove_deletes_ses_identity`), with the removal rows of the edge-case register: H11 (one
name's routing removed, the zone's others kept; a gone zone: `it::domains::h11_remove_keeps_zone_routing`),
H12 (a failed job restarted daily and by `DELETE`: `it::domains::h12_remove_failed_restart`), H13 (a re-added
name gets a new row and the old monitor is retired: `it::domains::h13_readd_new_row`), H14 (eviction and the
14-day expiry, which finish through removal: `it::domains::h14_unverified_claims`) and H16 (the hourly
cleanup of a failed add: `it::domains::h16_failed_add_cleanup`). The global retention job ([Privacy
§5.3](design/privacy.md#53-global-retention-job)) lands with its framework and the steps whose tables
have writers by now: `idempotency`, `platform_events`, `jobs`, `usage`, `dlq`, `signing_keys`, `staging`,
`held_erasures` and `audit` (`it::retention::global_job_steps`, `it::erasure::i2_hold_tenant_scope`). Its other steps are added by the milestones that write
their tables, each with its own test: `console` in M21, `billing_events` in M22, `ses_ingest` in M23,
`signup` in M24 and `identity_keys` in M25.

**Stubs completed by later milestones.** Tenant erasure ([Privacy §6.6](design/privacy.md#66-tenant-scope))
reaches tables and services that later milestones build. M14 writes every step, and leaves these substeps
as named stub functions in `jobs/erasure.rs` that do nothing until their milestone fills them in, with the
test it names:

| Substep | Completed by | Test |
|---|---|---|
| Console rows: `members`, `invitations` and `sessions` in `delete_d1_rows` | M21 | `it::erasure::tenant_console_rows` (lands in M21) |
| Billing: the `cancel_billing` step (step 2), and `billing_events` and `billing_accounts` in `delete_d1_rows` | M22 | `it::erasure::tenant_cancels_billing`; billing assertions in `it::erasure::tenant_console_rows` |
| SES: the domain's addresses in the `pm-retired-{n}` receipt rules (`prune_retired_rules`, run by `remove_domains`) | M23 | `it::erasure::tenant_ses_rows` (lands in M23) |
| Person rows: deleting every person left with no workspace ([Privacy §6.9](design/privacy.md#69-people-console-accounts)) | M24 | `it::erasure::person_scope`; assertions on people left with no workspace in `it::erasure::tenant_console_rows` |
| Notifier: `notification_prefs` in `delete_d1_rows`, and `Notifier` `delete_all` | M26 | Notifier assertions in `it::erasure::tenant_console_rows` and `it::erasure::person_scope` |
| Service accounts: `service_accounts` in `delete_d1_rows` (tenant scope) and in `scrub_control_plane` (identity scope) | M27 | `it::accounts::erasure_and_retention` |

---

## M15 · MCP server (after M9, M10, M11, M22, M25)

**Files:** `mcp/{mod.rs, transport.rs, tools.rs, schemas.rs, prompts.rs}`.

**Implements:** FR-MCP-1 and the tool list in [MCP reference](../reference/mcp.md), including the two
signing tools of M25 (`mail_sign_assertion`, `mail_sign_http_request`). M27 later adds
`mail_request_account` and `mail_list_accounts` to the same table and its table test.

**Acceptance:**

- `tools/list` is filtered by permission.
- Each tool's call maps to its REST equivalent, checked by a table test.
- Error mapping.
- Revision 2026-07-28 has no sessions: the server never mints `Mcp-Session-Id`, and `GET` and `DELETE`
  on `/mcp` answer `405`.
- `it::mcp::input_schemas_plain_objects`: every tool's `inputSchema` is a plain object at the top level,
  with no `oneOf`, `anyOf`, `allOf` or `not`.
- A recorded MCP Inspector session replays green.

---

## M16 · Rust SDK and CLI (Track 3, from M5; commands land as their APIs land)

**Files:** `crates/sdk/src/*`, `crates/cli/src/{main.rs, config.rs, output.rs, cloudflare/, commands/*}`.

**Implements:** FR-SDK-1, FR-CLI-1, FR-OPS-1–3. The SDK's `verify_assertion` and the CLI commands
`identity-keys`, `assertions` and `http-sign` land with M25's endpoints.

**Acceptance:**

- SDK integration tests run against the workerd harness for every endpoint, and
  `sdk::coverage::every_operation` proves the SDK has one method per `openapi.yaml` operation (FR-SDK-1,
  [Rust workspace §11](design/rust-workspace.md#11-the-rust-sdk-fr-sdk-1)).
- `idvy setup` is idempotent: it runs twice against a recorded Cloudflare API fake with no
  duplicate resources.
- `idvy deploy` verifies checksums and refuses a tampered bundle.
- `idvy doctor` reports every check with a fix.
- The landing-page CLI examples run as tests, and `sdk::doctest::published_examples` compiles the Rust
  examples of `quickstart.md` and checks that the landing page's example equals
  `crates/sdk/examples/confirm_booking.rs`.

---

## M17 · Observability and operations (Track 3)

**Files:** `crates/worker/src/{log.rs, metrics.rs}` (extending the minimal writers M5 created),
`ops/alerts.rs`, `consumers/dlq.rs`, `crates/core/src/slo.rs`
(alert rules, pure), `handlers/platform.rs`
(the platform API: `GET /v1/platform/dlq`, `POST /v1/platform/dlq/{dlq_id}/redrive`,
`POST /v1/platform/jobs`, `GET /v1/platform/jobs/{job_id}`, `POST /v1/platform/keys/{purpose}/rotate`, all
`platform:ops`; `GET /v1/platform/status` (`audit:read`), whose `master_key` block lands in M17
Foundation and the rest in M17 Completion; and in M17 Completion `PUT /v1/platform/switches`, `POST /v1/platform/identities/{identity_id}/restore` and
`POST /v1/platform/erasure-requests/{erasure_id}/reapply`),
`ops/{alert_email.rs, containment.rs, switches.rs, freeze.rs, restore.rs, capacity.rs}` (the restore
tooling: the freeze, mailbox PITR externs on `Storage::as_raw()` and the `restore_reconcile` job),
`crates/core/src/{sqldump.rs, schema.rs}`, CLI `ops status|switch|freeze|unfreeze|restore` and
`erasure retry|reapply` ([CLI and setup § 20](design/cli.md#20-ops)),
`.github/workflows/heartbeat.yml`, `crates/core/src/sealed.rs` (the registry of sealed columns, pure), `ops/reseal.rs` (the
re-seal sweep that the every-minute cron runs), CLI `dlq list|redrive` and `secrets rotate-master`. There is no
internal-only handler: the CLI uses the public platform API.

**Implements:** FR-OPS-4, NFR-OPS-2, NFR-OPS-3, NFR-COST-1 and [Observability](design/observability.md).

M17 is accepted in two halves, without renumbering ([Dependency graph](#dependency-graph)).

**Acceptance, M17 Foundation** (Track 3's first pull request; it merges before M7, M8 and M9):

- The log and metrics writer (`log.rs`, `metrics.rs`, created minimal in M5 and extended here):
  `event = "metric"` lines with the catalogued labels, and log scrubbing (part of I5).
- The alert table (the [alert list](design/observability.md#53-alert-list) as data in `ops/alerts.rs`:
  each alert's key, class, severity and runbook) and the state alert evaluator
  ([Observability §5.4](design/observability.md#54-the-state-alert-evaluator)): `core::slo::alert_state_machine`
  and `it::ops::alert_evaluator_transitions`. M9's G3 (`it::ops::provider_quota_80`) and M23's N26
  (`ses_identities_90pct`, checked by `it::ops::ses_alerts`) fire through them and are accepted there.
- The master-key rotation ([Security §6.2](design/security.md#62-rotation-procedures)):
  the two master-key slots (`PM_MASTER_KEY`, `PM_MASTER_KEY_B`, `PM_MASTER_KEY_ACTIVE`), the
  every-minute re-seal sweep over the registry in `crates/core/src/sealed.rs`, the `master_key` block of
  `GET /v1/platform/status` (whose `remaining` count is built from the same registry) and
  `idvy secrets rotate-master`:
  `it::secrets::master_key_rotation` and `cli::secrets::rotate_master` for the columns registered so far.
  Each milestone that writes a sealed column registers it and extends the test
  ([Shared files](#dependency-graph)).

**Acceptance, M17 Completion** (accepted at M20, once what it measures has landed):

- J8, J3 (job start through `POST /v1/platform/jobs`; it needs M14's `reparse` job),
  `it::secrets::signing_key_rotation`.
- Every metric in the design is emitted by at least one test path (`it::ops::metrics_emitted`).
- Every SLO of [Observability §4](design/observability.md#4-service-level-objectives) is computed from emitted metrics
  (`it::ops::slo_from_metrics`): NFR-REL-1–4, NFR-PERF-1–6 and NFR-PRV-1, including those that need
  later milestones (NFR-PRV-1 needs M14's erasure, NFR-PERF-6 M11's agentic search).
- NFR-OPS-2, I6 and the restore tooling: `it::ops::freeze_holds_everything`,
  `it::ops::restore_replays_changes`, `it::ops::restore_rebuilds_ledger`, `it::erasure::i6_reapply_by_hash`,
  `cli::ops::sqldump_parses_export`, `cli::ops::restore_replay_diff`, `cli::ops::freeze_unfreeze`, and the
  restore runbook that `live::ops::restore_drill` runs in M20.
- NFR-OPS-3 and the solo-operator controls ([ADR 0015](adr/0015-solo-operator.md)): alert email and the
  heartbeat (J32: `it::ops::j32_alert_channels`, `it::ops::heartbeat_missing`), the break-glass key
  (J36: `it::ops::j36_break_glass_key`), automatic containment
  (J33: `it::ops::j33_auto_containment`), `job_failed:{kind}` (`it::ops::job_failed_alerts`), the
  per-domain bounce and complaint state alerts (`it::ops::domain_rate_state_alerts`), capacity (J30:
  `it::ops::j30_d1_capacity`, J31: `it::ops::j31_vector_capacity`), `GET /v1/platform/status`
  (`it::ops::platform_status`), `cli::erasure::retry`, and the audit of platform- and partner-key mail
  reads (`it::security::mail_reads_audited`). The heartbeat workflow runs against staging in M20
  (`live::ops::heartbeat_workflow`).
- NFR-COST-1: the generated `wrangler.toml` declares no always-on compute (no Containers, no binding
  that bills while idle beyond storage), checked by `xtask::template_no_idle_compute`; the idle-cost
  review runs in M20.

---

## M18 · Quality gates (after M10–M12)

**Files:** `crates/conformance/golden/` (a generator for about 5,000 synthetic messages plus labelled
queries), `xtask eval-setup`, `xtask eval-search`, `xtask eval-agentic`, `xtask eval-triage`.

**Implements:** NFR-QUAL-1–3.

**Acceptance:** recall@10 ≥ 0.90 (hybrid), citation precision ≥ 0.98, and triage accuracy ≥ 0.85.
These figures are measured on the golden set using real Workers AI models in a nightly CI job, in the
separate evaluation account with the token and the freshly created `pm-mail-chunks-eval` index of
[Testing § 9.2](design/testing.md#92-running) (`cargo xtask eval-setup`, then the REST paths with
`PM_EVAL_REST = "on"`), and recorded in `docs/src/project/quality.md` (created by this milestone). Each
suite runs three times and is scored by the median (six runs when the spread exceeds 0.02; still above
it, the job fails as `unstable`); CI fails when the median drops more than 1 point below the baseline,
itself the median of five runs. `xtask::eval_median_policy` checks the policy on recorded per-run
results.

---

## M19 · Site, docs and release pipeline (Track 4; the release half after M16)

**Files:** `site/` (exists, with Wrangler pinned in `site/package.json` and `site/package-lock.json`),
`.github/workflows/{site.yml (exists), release.yml, release-publish.yml, cloud-deploy.yml}`, `release-gates/`,
`deploy/wrangler/{package.json, package-lock.json}`, and the xtask commands `release`, `release sign`,
`release crates` and `release-gate`.

**Implements:** the release pipeline of [Rust workspace § 10](design/rust-workspace.md#10-ci-pipeline),
which is its one definition, and PRD release criterion 7.

**Acceptance:**

- `mdbook build docs` writes into `site/public/docs`, and the site Worker serves both. The landing
  page's links to docs anchors resolve (link check in CI).
- `site.yml` installs Wrangler with `npm ci --ignore-scripts` from the committed lockfile and deploys
  with the per-Worker token of the `production` environment
  ([Security § 11](design/security.md#11-supply-chain)).
- `xtask::release_version_matches_tag` and `xtask::release_gate_refuses` pass.
- The first release candidate, `v1.0.0-rc.1`, is tagged once M16 has landed. Its `release.yml` run
  produces a draft GitHub Release with `pylota-identity-worker-1.0.0-rc.1.tar.gz`, CLI binaries for macOS
  (arm64, x64), Linux (x64, arm64) and Windows (x64), the SBOMs, `SHA256SUMS` and an attestation for every
  file, and no signature. `cargo xtask release sign v1.0.0-rc.1` with the offline key adds
  `SHA256SUMS.sig` with the trusted comment `pylota-identity v1.0.0-rc.1` and starts `release-publish.yml`,
  whose `verify` job makes it a pre-release and whose `staging` job deploys it with
  `idvy deploy --version 1.0.0-rc.1`. Its `live` job may fail before M20; a failed run is never
  promoted, and a pre-release never reaches crates.io.
- `xtask::release_sign_refuses`: `cargo xtask release sign` refuses a file whose checksum does not match
  or whose attestation is missing or names another workflow or tag, and uploads nothing.
- `cloud-deploy.yml` refuses a pre-release version, a binary whose signature or checksum does not verify
  against the keys written in the workflow, and an attestation from another workflow or tag;
  `xtask::cloud_deploy_workflow_pins` and `xtask::release_crates_refuses` pass; immutable releases are on
  for the repository; no workflow holds the crates.io token (J35).
- `cli::wrangler::installs_from_signed_lockfile`: the bundle carries Wrangler's lockfile and `idvy`
  installs and runs Wrangler only from it (J34).
- The site serves `/terms`, `/privacy`, `/dpa`, `/subprocessors` and `/trademarks` from pages counsel and the
  owner supplied ([Launch readiness §4](launch-readiness.md#4-legal-pages-and-processors)); the landing page
  links each from its footer; a link check covers them.
- `xtask::release_artefacts_carry_licence`: every archive and package the release produces (the Worker
  bundle, the CLI archives, each `.crate`) contains its licence file (`LICENSE.md`, or `LICENSE` and `NOTICE` for
  the Apache-2.0 crates of [ADR 0022](adr/0022-sdk-languages.md)) and a `THIRD_PARTY_LICENSES` file generated
  from its SBOM; `cargo deny check licenses` is green.
- The release notes and `CHANGELOG.md` state each version's release date, from which its Apache-2.0 date
  (the second anniversary) is computed and printed beside it.
- A `dco` CI job refuses a pull request whose commits lack a `Signed-off-by` line matching the author
  (`xtask::dco_refuses_unsigned`).
- `xtask::docs_forbidden_claims` scans `site/public/index.html` copy as well as `docs/` and refuses "open
  source" as a description of Pylota Identity (an `id` attribute is not copy).

---

## M21 · Console and workspaces (after M7–M14 and M25, whose services its pages use)

**Files:** `crates/worker/src/console/{mod.rs, router.rs, session.rs, signin.rs, csrf.rs, layout.rs (maud), pages/*.rs}`,
`crates/worker/src/members/{mod.rs, invitations.rs, roles.rs}`, `handlers/members.rs`. No migration:
`users`, `members`, `invitations`, `login_tokens` and `sessions` are in `0001_init.sql`.

**Implements:** FR-CON-1–7, NFR-CON-1, the console screens in [Console design](design/console.md),
including identity-key management on the identity page ([Agent signing keys §6](design/agent-keys.md#6-permissions-limits-and-plans)).

**Acceptance:**

- Edge rows W9, W10, W15–W18, W35 (`it::keys::w35_tenant_erase_owner_only`, and the tenant-erasure row
  of `it::console::w18_role_and_scope`: the owner's session principal holds `tenants:erase`, an admin's
  never) and W36 (`it::members::w36_removed_member_keys_revoked`: the console's key form records the
  person and role, and removal, leaving and role changes revoke their keys), W47
  (`it::members::w47_removed_member_endpoints_disabled`) and W48 (`it::keys::w48_rotation_provenance`).
- J18 (`it::partners::j18_partner_limits`), accepted here with its invitation case, which counts in
  `RL_PARTNER` with tenant creations; its `max_tenants` part runs from M5 and its erased-tenant case from
  M14.
- `it::console::attachments_and_inline_images`: the console serves attachments and inline images
  itself, so a page never loads anything from the API host.
- Every console route works with JavaScript disabled, checked by the browser suite
  (`browser::console::no_js`, Playwright with `javaScriptEnabled: false`), which `cargo xtask itest` runs
  from this milestone on ([Testing §2](design/testing.md#2-test-layers)).
- An axe accessibility scan finds no violation of impact `serious` or `critical` on any page
  (`browser::console::axe_scan`).
- NFR-CON-1: `it::console::render_budget` keeps server render time p95 ≤ 300 ms on every page.
- Sign-in and invitation emails are sent through the system identity that setup creates
  ([Identities and domains › The system identity](design/identity-domains.md#the-system-identity)), using the
  simulator in tests; invitations also work with `PM_CONSOLE=off`. The system identity is exempt from the
  tenant daily cap and from abuse auto-pause (`it::send::system_identity_exemptions`).
- Erasure extension: the console-rows stub of tenant erasure (`members`, `invitations` and `sessions` in
  `delete_d1_rows`, [Privacy §6.6](design/privacy.md#66-tenant-scope)) is filled in, and
  `it::erasure::tenant_console_rows` lands here, asserting those rows; M22, M24 and M26 add their
  assertions to it (M23's SES rows have their own test, `it::erasure::tenant_ses_rows`).
- The global retention job's `console` step (`login_tokens`, `sessions`, and invitations expired or
  revoked more than 30 days ago) is added here: `it::retention::global_console_rows`.
- The workspace policy page `/console/settings/policy` ([Workspace policy §6](design/workspace-policy.md#6-the-console-page)),
  on M5's `policy::write` and `policy::view`: `it::console::policy_page`; owners and admins hold
  `policy:write` in their session's permission set.

---

## M22 · Plans, metering and billing (after M9, M12, M21)

**Files:** `crates/worker/src/billing/{mod.rs, catalog.rs, quota.rs (TenantQuota allowances and holds), stripe.rs, webhook.rs, usage.rs}`,
`handlers/{usage.rs, plans.rs, billing.rs}` (`GET /v1/usage` and `GET /v1/usage/daily`),
console pages `plan.rs`, and the dispute check in outbound policy step 18 (`details.cap:
"billing_dispute"`). No migration: `billing_accounts` (with `dispute_open_at`) and `billing_events` are in
`0001_init.sql`.

**Implements:** FR-BILL-1–12, FR-BILL-14, NFR-BILL-1/2, the metering points in [Billing design](design/billing.md).

**Acceptance:**

- Edge rows W1–W8, W11–W14, W19, W37–W39 and W49 (the `ramp_lifted_at` assertions of W37 and W38 are
  added by M24, which owns the ramp), and `it::billing::unresolved_event_ignored`.
- The parts of M5's tests that need this milestone's routes: the `GET /v1/usage` part of
  `it::keys::permission_level_rules` (implicit `usage:read` for tenant and identity keys; a platform key
  needs it listed and `tenant_id`) and the `PATCH /v1/tenants/{own}/billing` case of
  `it::partners::routes_and_audit` (`403 scope_denied` for a partner key).
- `xtask::stripe_setup_idempotent`: `cargo xtask stripe-setup` creates the products, prices, the three
  Portal configurations and the webhook endpoint once, and writes a catalog that `idvy deploy`
  accepts ([Billing › Stripe account setup](design/billing.md#stripe-account-setup)).
- Every metered action is wired to a hold and a settlement, checked by a table test that lists each metering
  point. A new metered action without a row fails.
- A Stripe test-mode run (CLI `stripe trigger` fixtures recorded as JSON) covers checkout completed,
  subscription updated, payment failed, and canceled.
- `GET /v1/usage` matches the catalog and the `TenantQuota` state in property tests.
- NFR-BILL-1: `it::billing::w1_last_unit_race` and the hold property tests allow 0 actions beyond a
  granted allowance. NFR-BILL-2: `it::billing::w2_stripe_down_sends_ok` fails no metered action while
  Stripe is unreachable.
- Erasure extension: the billing stub of tenant erasure, that is the `cancel_billing` step (step 2, right
  after routing stops: the plan and every top-up subscription cancelled at once with `invoice_now=true`
  and `prorate=false`, so no credit and no refund) and `billing_events` and
  `billing_accounts` in `delete_d1_rows` ([Privacy §6.6](design/privacy.md#66-tenant-scope)), with
  `it::erasure::tenant_cancels_billing`; webhooks for an erased tenant are answered `200` and recorded
  `ignored_erased`, except that a live subscription created after the deletion is cancelled
  (`it::billing::late_subscription_after_erasure`). `it::erasure::tenant_console_rows` gains the billing assertions.
- The global retention job's `billing_events` step: `it::retention::global_billing_events`.

**Gate:** every request pins `Stripe-Version: 2025-03-31.basil`, and each Stripe call matches the
`Verified` line of [Billing](design/billing.md#tests) (read 2026-10-10; re-read and update it if it is more
than 30 days old when the code is written).

---

## M24 · Cloud sign-up and sign-in (after M21, M22)

**Files:** `crates/worker/src/console/{signup.rs, oauth.rs, totp.rs, landing.rs, onboarding.rs, pages/overview.rs}`,
`crates/core/src/totp.rs` (RFC 6238 codes, pure; the console module only stores and checks them),
`handlers/platform.rs` (`POST /v1/platform/waitlist/invite`), no migration (the `users` sign-in
columns, `oauth_identities`, `oauth_states`, `pending_auth`, `waitlist`, `platform_state` and
`tenants.{require_two_factor, onboarding_dismissed_at, ramp_lifted_at}` and the `login_tokens` sign-up
columns are in `0001_init.sql`), the pending step (`console/pending.rs`, [Cloud sign-up
§5.1](design/cloud-signup.md#51-the-pending-step)), the host split for `PM_CONSOLE_HOST` in `router.rs`,
CLI `idvy waitlist invite`, the new-workspace send ramp (`crons/signup_ramp.rs`, run once a day by the
`*/15` cron; the ramp check in outbound policy step 18; the `ramp_lifted_at` updates in
`billing/webhook.rs`; `QuotaRequest::OutcomeRates`), the system-mail budgets and the form ticket
(`console/sysmail.rs`, `QuotaRequest::SystemMail`), the shared-domain breaker (`crons/send_breaker.rs`
and its check in outbound policy step 18), and person deletion (`console/pages/settings.rs` and the
person step of `jobs/erasure.rs`).

**Implements:** FR-CON-8–13, FR-CON-16–18, [Cloud sign-up, sign-in and first run](design/cloud-signup.md).

**Acceptance:**

- Edge rows W40–W46 (W43's `sign_in_codes_locked` email assertion is added by M26), with
  `it::members::w40_invitation_needs_second_factor`, `it::totp::w41_pending_auth_single_use`,
  `it::oauth::w42_link_needs_code`, `it::console::w43_failed_code_daily_cap`,
  `it::abuse::w44_system_mail_budgets`, `it::console::w45_hosted_address_refused` and
  `it::abuse::w46_shared_domain_breaker`, and the ramp assertions of `it::billing::w37_grant_after_payment`
  and `it::billing::w38_dispute_and_refund`.
- Edge rows W20–W34, with the tests named in the register (`it::oauth::*`, `it::signup::*`, `it::totp::*`,
  `it::landing::routing_table`, `it::checkout::*`, `it::abuse::free_ramp`, `it::abuse::ramp_evaluator`,
  `it::abuse::partner_ramp` (W30's partner part: a partner's tenants are ramped unless `ramp_exempt`),
  `it::console::delete_account_owner_required`).
- Erasure extension: person deletion ([Privacy §6.9](design/privacy.md#69-people-console-accounts)) is
  owned here. That covers account deletion at `/console/settings`, the person-rows stub of tenant
  erasure (every person left with no workspace), the scrub of accepted invitations, and the
  system-mail counterparty erasure restricted by the internal `identity_ids` param
  ([Privacy §6.4](design/privacy.md#64-counterparty-scope-i1)). Tests: `it::erasure::person_scope`,
  `it::privacy::system_mail_retention_and_person_delete` and `it::erasure::tenant_console_rows`'s
  assertions on people left with no workspace.
- `core::totp::rfc6238_vectors`, `it::signup::email_creates_account_only_on_use`,
  `it::onboarding::derived_steps` and `it::hosts::console_api_split`.
- The new pages pass the M21 checks: they join `browser::console::no_js` and `browser::console::axe_scan`
  (no JavaScript needed, no axe violation of impact `serious` or `critical`).
- `users.totp_sealed`, `users.totp_pending_sealed`, `users.recovery_codes_sealed` and `oauth_states.pkce_sealed` are registered in
  `crates/core/src/sealed.rs`, so M17 Foundation's re-seal sweep covers them, with their cases in
  `it::secrets::master_key_rotation`.
- The global retention job's `signup` step (`oauth_states`, `pending_auth`, `waitlist`):
  `it::retention::global_signup_rows`.

**Gate:** Google's and GitHub's endpoints and claim names are re-read from their current documentation
and recorded in the design before the OAuth code is written ([Cloud sign-up §4](design/cloud-signup.md#4-google-and-github)).

---

## M25 · Agent signing keys, assertions and signed requests (after M5 and M6; S13 gates signed requests)

**Files:** `crates/core/src/{jwk.rs, jwt.rs, httpsig.rs}` (RFC 7638 and RFC 8037 thumbprints and JWS,
RFC 9421 signature bases, pure), `handlers/{identity_keys.rs, assertions.rs, http_signatures.rs,
well_known.rs}`, `db/identity_keys.rs`, `keyring.rs` (the `web_bot_auth` purpose: a 43-character
thumbprint kid, `public_jwk`, the 7-day directory overlap), the use of the `RL_SIGN` binding (in the
template since M5), `crates/sdk/src/assertions.rs` (`verify_assertion`), CLI `idvy identity-keys
list|create|rotate|revoke`, `idvy assertions create|verify` and `idvy http-sign`; no migration
(`identity_keys`, `key_tombstones` and the `signing_keys` columns are in `0001_init.sql`). Route
registrations go through Track 1 as usual.

**Implements:** FR-IDN-6–9 and edge rows O1–O13 ([Agent signing keys and signed requests](design/agent-keys.md)),
and the cross-tenant suite's new routes (NFR-SEC-1).

**Tasks:**

1. Write the RFC vector tests first (`core::jwk`, `core::jwt`, `core::httpsig`), then the pure code.
2. Identity keys: lazy creation on first sign, `POST …/keys`, rotation with the
   `PM_IDENTITY_KEY_OVERLAP_DAYS` overlap, revocation, the `key_tombstones` check at generation, and the
   three `identity.key_*` events through the identity's mailbox (`MailboxRequest::EmitEvent`).
3. The JWKS endpoint, with the pause and suspension kill switch.
4. Assertions (`identities:sign`, `RL_SIGN`), never stored or logged; `Idempotency-Key` ignored. Each
   signature is counted through `QuotaRequest::RecordUsage` (`usage:assertions`, `usage:http_signatures`,
   flushed to `usage_daily`; the M5 stub has recorded `RecordUsage` since M5, so the calls work whatever lands first).
5. Signed HTTP requests and the signed directory behind `PM_WEB_BOT_AUTH` and tenant policy
   `web_bot_auth.allowed`; the `web_bot_auth` purpose of `POST /v1/platform/keys/{purpose}/rotate`.
6. The SDK verifier and the CLI commands; the two MCP tools are added to M15's table.

**Acceptance:**

- O29 (`it::assertions::o29_vouched_address`).
- O27 (`it::assertions::sdk_verifies` with its crafted-`sub`, redirect and content-type cases) and O28
  (`it::identity_keys::o28_concurrent_first_sign_and_revoke_active`); the `RL_SIGN` case of
  `it::auth::rate_limited` (the 601st signing call in a minute for one identity); tenant and identity keys
  minted by a platform key and by a partner key with `identities:sign` granted sign successfully (the
  grant cases of `it::keys::permission_level_rules` and `it::keys::j11_partner_key_limits`).
- Edge rows O1–O13, with the tests named in the register: `core::httpsig::signature_base_rfc9421` (O10),
  `it::identity_keys::{lazy_create_and_rotate, revoke_removes_from_jwks, paused_withdraws_jwks}` (O2, O3,
  O1), `it::assertions::claims_and_limits` (O4–O6), `it::secrets::rotate_master_reseals_identity_keys`
  (O8: M25 registers `identity_keys.private_enc` in `crates/core/src/sealed.rs`, so M17 Foundation's
  re-seal sweep covers it alongside `signing_keys.ciphertext`, and adds its case to
  `it::secrets::master_key_rotation`; if M25 lands before M17 Foundation, M17 Foundation does both and
  this test runs once it has landed),
  `it::http_signatures::{disabled_and_policy, expiry_bounds}` (O9, O13, O11) and
  `it::well_known::directory_signed_per_key` (O12). O7 (`it::assertions::erasure_tombstones_kid`) runs
  once M14 has landed too: M14's identity- and tenant-scope erasure deletes the keys and writes
  `key_tombstones`.
- The global retention job's `identity_keys` step, which retires `retiring` keys past `verify_until`:
  `it::retention::global_identity_keys` (it runs once M14 has landed; if M14 lands later, M14 writes the
  step).
- `core::jwk::thumbprint_rfc8037_vector`, `core::jwt::eddsa_rfc8037_vector` and
  `it::assertions::sdk_verifies` (the SDK verifier accepts a fresh token and rejects a wrong audience, an
  expired token, an unknown kid and `alg: none`).
- The cross-tenant suite covers the six new `/v1` routes; another tenant's key gets the same `404` as a
  missing identity, and the identity JWKS route (`/.well-known/jwks/{identity_id}.json`) gives the same
  `404 identity_not_found` for unknown, paused and deleted identities. The key directory is deployment-wide;
  its only `404` is `key_not_found`, while `PM_WEB_BOT_AUTH` is `off`.
- No response, log line, event or idempotency record contains a private key, a seed, an assertion or a
  signature (`it::logs::i5_no_content_in_logs` is extended with them).

**Gate:** signed HTTP requests ship only when spike S13 passed. Otherwise `PM_WEB_BOT_AUTH` cannot be
turned on (`422 web_bot_auth_disabled`, the directory `404`), FR-IDN-8 moves to v1.1 by ADR, and the
assertion half of the milestone ships unchanged.

---

## M26 · Notifications and usage alerts (after M9, M10, M21, M22 and M24; the system identity from M6)

**Files:** `crates/worker/src/notify/{mod.rs, notifier.rs (the Notifier Durable Object), compose.rs,
prefs.rs, unsubscribe.rs}`, `crates/core/src/notify.rs` (windows, caps, schedules across time zones, and
rendering that takes no mail content, pure), `crates/worker/src/console/pages/notifications.rs`
(`/console/settings/notifications` and the unsubscribe pair), the `Notifier` class replacing M5's stub
(the `NOTIFY` binding and the class name are in the template and `export_worker!` since M5), and
`tenants.notify_do_id` minted with the
tenant row, plus a `Notifier` minted by the every-minute cron for each tenant still at
`notify_do_id = ''` (those created before M26, setup's default tenant included;
[Configuration › Bindings](../reference/configuration.md#bindings)); no migration (`notification_prefs` and the
column are in `0001_init.sql`). Hooks in other milestones' files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `consumers/webhooks.rs` (M8's dispatcher) | `NotifierRequest::Event` for `message.received`, `message.released` and `message.triaged` |
| `consumers/delivery.rs` (M9's delivery-event consumer, which SES events also reach) | After a hard bounce or complaint on a system-identity message carrying `metadata.notify_user_id`, set `paused_reason` on every `notification_prefs` row of that person ([O17](edge-cases.md); [Outbound › Applying an event](design/outbound.md#applying-an-event-to-a-message)) |
| `quota/mod.rs` (M22's `TenantQuota`) | `NotifierRequest::UsageThreshold` |
| `members/mod.rs` and `handlers/members.rs` (M21) | `NotifierRequest::MemberRemoved` on removal and leaving; `Account { event: ownership_transferred }` on a transfer |
| `console/totp.rs` (M24) | `Account { event: two_factor_disabled }` |
| `console/pending.rs` (M24) | `Account { event: sign_in_method_linked }` when the emailed code links a provider identity |
| `console/signin.rs` (M21) | `Account { event: sign_in_codes_locked }` when an address reaches 30 failed codes in a UTC day |
| `billing/webhook.rs` (M22) | `Account { event: payment_failed }` when the status becomes `past_due` |
| `jobs/erasure.rs` (M14) | The Notifier stub of tenant erasure (`notification_prefs`, `Notifier` `delete_all`), and the `notification_prefs` rows of person deletion |

**Implements:** FR-CON-14, FR-CON-15, FR-BILL-13 and edge rows O14–O26
([Notifications and usage alerts](design/notifications.md)).

**Tasks:**

1. `core::notify` first: coalescing windows, caps, the 09:00 schedule per time zone, and the
   content-free renderer, each unit-tested.
2. The `Notifier` object (`Init`, `pending`, `held`, `windows`, `sent`, `meta`, one alarm) and its inputs:
   the dispatcher hook for `new_mail` (with `held` for the `needs_reply` filter), `UsageThreshold` from
   `TenantQuota` (with the `alerted:{feature}:{threshold}:{period}` keys), `Account` from the code that
   changes security or billing state, and `MemberRemoved`; the daily `digest` of capped items.
3. Sending through the system identity with the `notify:` idempotency key; bounces and complaints set
   `paused_reason`; the hourly retry loop for a failing platform domain and for a refused system-identity
   submit, with the `system_mail_blocked` alert.
4. The console settings page, the bounce banner and its confirmation, and the unsubscribe pair (no
   session, CSRF-exempt, served with `PM_CONSOLE=off`).
5. Member removal and person and tenant erasure delete preferences and pending items: the erasure
   extension fills the Notifier stub of tenant erasure ([Privacy §6.6](design/privacy.md#66-tenant-scope))
   and adds the `notification_prefs` rows to person deletion (§6.9).

**Acceptance:**

- Edge rows O14–O26, with the tests named in the register (`it::notify::*`).
- `core::notify::no_content_in_body`: a rendered notification contains no subject, sender, snippet or
  attachment name from the source message.
- Notification emails go out through the system identity with the simulator in tests, as
  `transactional` sends carrying `List-Unsubscribe` and `List-Unsubscribe-Post`; a retried alarm never
  sends twice (the `notify:` idempotency key).
- The new pages pass the M21 checks: they join `browser::console::no_js` and `browser::console::axe_scan`
  (no JavaScript needed, no axe violation of impact `serious` or `critical`). The unsubscribe pair works
  without a session and with `PM_CONSOLE=off`.
- The cross-tenant suite covers the unsubscribe route: a token never changes another person's or
  workspace's preferences (O18).
- `it::notify::system_mail_blocked_retries`, and `it::console::account_emails` for all five `account`
  events; the `sign_in_codes_locked` assertion of `it::console::w43_failed_code_daily_cap`.
- `it::notify::unsubscribe_token_format`: the token layout of
  [Notifications § 5](design/notifications.md#5-the-emails) round-trips and every malformed token is
  refused.
- Erasure extension: `it::erasure::tenant_console_rows` and `it::erasure::person_scope` gain their
  `notification_prefs` and `Notifier` assertions, and `it::notify::member_removed_drops_pending` covers
  `held` rows.

---

## M27 · Service sign-up ledger (after M7, M8, M14, M15, M16 and M21)

**Files:** `crates/core/src/accounts.rs` (request normalisation, the match rule and the gate, pure),
`crates/worker/src/accounts/{mod.rs, service.rs}`, `handlers/accounts.rs` (the eight `…/accounts`
routes), `console/pages/accounts.rs`; no migration (`service_accounts`, `tenants.partner_ceilings_json`,
the mailbox's `verifications.account_id` and the `account_unapproved` quarantine reason are in
`0001_init.sql` and mailbox schema v1). Hooks in other milestones' files, each reviewed by that file's
owner:

| File | Hook |
|---|---|
| `consumers/inbound.rs` (M7) | Step 12: the approved-entries query when `accounts.require_approval` is on and the message has a verification match; `IngestInput.approved_accounts` |
| `mailbox/ingest.rs` (M7) | Quarantine rule 4a (`account_unapproved`), and `verifications.account_id` |
| `handlers/wait.rs` (M7) | The `403 policy_denied` (`account_not_approved`) check for `kind=verification`, and the entry re-check before a release |
| `jobs/erasure.rs`, `jobs/retention.rs` (M14) | The `service_accounts` rows of identity and tenant erasure; the global retention job's `service_accounts` step |
| `mcp/tools.rs` (M15) | `mail_request_account` and `mail_list_accounts`, with their rows in M15's table test |
| `crates/cli/src/commands/accounts.rs` (M16) | `idvy accounts request\|list\|get\|approve\|reject\|close\|delete` |
| `console/pages/overview.rs` (M24) and the Notifier's needs-a-person counts (M26) | The pending sign-ups item of "Needs a person" |

**Implements:** FR-IDN-10 and edge rows E9–E15, with E5's ledger part
([Service sign-up ledger](design/service-accounts.md)), and the cross-tenant suite's new routes (NFR-SEC-1).

**Acceptance:**

- Edge rows E9–E15, with the tests named in the register: `it::accounts::e9_unapproved_code_held` (E9, and
  E5's ledger part), `core::accounts::e10_match_rules` and `it::accounts::e10_matched_code_released` (E10),
  `it::accounts::e11_key_approval_rule` (E11), `it::accounts::e12_limits_and_expiry` (E12),
  `it::accounts::e13_closed_entry_holds_codes` (E13), `it::accounts::e14_own_deployment_refused` (E14)
  and `it::accounts::e15_mailbox_provider_refused` (E15).
- `core::accounts::normalise_request`, `it::accounts::lifecycle_events_and_audit` (the four `account.*`
  events delivered through M8's fan-out to tenant, partner and platform endpoints) and
  `it::accounts::scope_matrix`; the eight routes join `it::security::cross_tenant_matrix`.
- Erasure and retention: `it::accounts::erasure_and_retention` fills the M14 stub row and adds the
  `service_accounts` step of the global retention job.
- `it::console::accounts_page`; the page joins `browser::console::no_js` and `browser::console::axe_scan`.
- `it::mcp::accounts_tools`; `sdk::coverage::every_operation` still passes with the eight new operations.
- With `accounts.require_approval` off, the M7 suites (`it::wait::e4_*`, `it::inbound::e5_unsolicited_otp`)
  pass unchanged.

---

## M29 · Drafts, approvals and scheduled send (after M8, M9, M14, M15, M16, M21, M22, M24 and M26)

**Files:** `crates/core/src/drafts.rs` (the field table per kind, the state table, `approval_needed`,
`digest`, the etag, the `send_at` window, the patch merge; pure), `crates/api-types/src/drafts.rs`,
`crates/worker/src/handlers/drafts.rs` (the ten routes), `crates/worker/src/mailbox/drafts.rs`
(storage, transitions, `SendDraft`, the `drafts` and `draft_index` alarm purposes),
`crates/worker/src/drafts/attachments.rs`, `crates/worker/src/send/{context.rs, authority.rs}`,
`crates/worker/src/crons/drafts_due.rs`, `crates/worker/src/console/pages/drafts.rs`,
`crates/cli/src/commands/drafts.rs`. No migration while nothing is built: `draft_index` goes into
`0001_init.sql`, and `drafts`, `draft_sync`, `messages.draft_id` and `idempotency.draft_id` into mailbox
schema v1 ([Drafts §9](design/drafts.md#9-storage)). Hooks in other milestones' files, each reviewed by
that file's owner:

| File | Hook |
|---|---|
| `mailbox/submit.rs` (M9) | `SubmitInput.draft` and `SubmitInput.approved`; stored draft attachments (an object, size, SHA-256 and `upload_id`) in steps 12, 17, 4, 6 and 7; the draft arm of the accept transaction; policy step 16a |
| The quarantine release (M7) and the screening verdict (M31) transactions | Re-sync the `draft_index` rows of drafts whose source changed state (`source_held`, Drafts § 9.3); M31 adds its half if it lands after M29 |
| `handlers/send.rs` (M9) | The D1 context loader moves to `send/context.rs`, unchanged, and gains the step-16a facts |
| `crates/core/src/policy.rs` (M2, M9) | Known-recipient rule 4; step 16a; `FIELDS` gains `drafts.require_approval` (guard) and `drafts.key_approval` (platform or own partner) |
| `policy/write.rs` (M5), `handlers/identities.rs` (M6) | The guard rule for `drafts.require_approval`; the stricter-only rule for `send_policy.require_approval` |
| `auth/people.rs` (M21) | `key_may_decide` gains `Decision`, with `ApproveDraft` reading `drafts.key_approval` |
| `webhooks/payloads.rs` (M8), `mailbox/outbox.rs` (M6) | `draft_summary`, `draft_event`; the audit rows of `draft.approved` and `draft.rejected` in the dispatch batch |
| `jobs/erasure.rs`, `jobs/retention.rs` (M14) | The draft rows of every scope, `drafts_deleted` in the receipt; draft purge and pending expiry in `Hygiene` |
| `mcp/tools.rs` (M15) | The six draft tools, with their rows in M15's table test; one sentence in the three send tools' descriptions |
| `crates/cli/src/commands/drafts.rs` (M16) | `idvy drafts …` |
| `console/pages/overview.rs` (M24) and the Notifier's needs-a-person counts (M26) | The two "Needs a person" items |
| The every-minute cron (M5, M17 Foundation) | `crons/drafts_due.rs` |
| `drafts/attachments.rs`, upload arm | Built by whichever of M29 and M33 (uploads) lands second, with `it::drafts::q6_upload_references` |
| The draft field table (`core::drafts`) | M32 (calendar invitations) adds the `calendar` field to `new` and `reply` drafts if it lands after M29 |

**Implements:** FR-DRF-1 … FR-DRF-14, the changes to FR-OUT-3, FR-OUT-13, FR-CON-5, FR-CON-12, FR-CON-15
and FR-PRV-3, and edge rows Q1–Q64, Q109 and Q110 ([Drafts](design/drafts.md)); the cross-tenant suite's
new routes (NFR-SEC-1).

**Acceptance:**

- Edge rows Q1–Q64, Q109 and Q110 with the tests named in the register, among them
  `it::drafts::q12_concurrent_sends_one_message` and `it::drafts::q32_claim_expiry_no_duplicate` (one
  message per revision), `it::drafts::q19_approval_binds_content`, `it::drafts::q27_direct_send_approval_required`,
  `it::drafts::q35_alarm_multiplexing`, `it::drafts::q37_missed_alarm_catch_up`,
  `it::drafts::q38_late_limit_and_transient` and `it::drafts::q39_fire_outcomes`.
- `core::drafts::{kind_field_rules, state_table, approval_levels, digest_canonical, etag_and_send_at,
  on_behalf_of_and_client_id}`.
- Erasure and retention: `it::drafts::q46_erasure_scopes` and `it::drafts::q48_retention` fill M14's
  rows.
- `it::console::drafts_page`; the pages join `browser::console::no_js` and `browser::console::axe_scan`.
- `it::mcp::drafts_tools`, `cli::drafts::send_key_derivation`; `sdk::coverage::every_operation` still
  passes with the ten new operations; the routes join `it::security::cross_tenant_matrix`.
- With `drafts.require_approval: never` and no draft, the M9 send suites (`it::send::*`) pass unchanged.
- At M20: `live::drafts::scheduled_send`.

---

## M30 · Event feed and live stream (after M8, M14, M15, M16, M17 Foundation and M21)

**Files:** `crates/core/src/events/{mod.rs, cursor.rs, visibility.rs, filter.rs, stream.rs, token.rs}`
(pure); `crates/api-types/src/events/feed.rs`; `crates/worker/src/events/{mod.rs, object.rs, log.rs,
consumers.rs, stream.rs, tokens.rs, schema/}` (the `TenantEvents` class, replacing M5's stub);
`handlers/events.rs` (the ten routes); `jobs/event_log_rebuild.rs`; `crates/platform/src/cf/hibernation.rs`
and `crates/platform/src/fakes/hibernation.rs`; `crates/sdk/src/events.rs` (feature `stream`);
`crates/cli/src/commands/events.rs`. `tenants.events_do_id` and the `event_log_rebuild` job kind are in
`0001_init.sql`; rows still at `''` get their object from the every-minute cron. Hooks in other milestones'
files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `crates/platform/src/cf/glue.rs` and `export_worker!` (M4, M5) | The three hibernation handlers exported for `TenantEvents`; `ObjectApp`'s provided WebSocket methods; `HttpResponseOut`'s upgrade variant; `DurableObjects::upgrade` |
| `auth/routes.rs` (M5) | The ten route entries; `Auth::KeyOrStreamToken` for the stream route |
| `consumers/webhooks.rs` (M8) | The append step before endpoint resolution, its age-based retry, and the Notifier hand-off reading step 1's payloads |
| `handlers/keys.rs`, `handlers/tenants.rs`, `handlers/partners.rs` (M5) | The best-effort `ForgetKeys` and `CloseStreams` calls after revocation and suspension |
| `jobs/erasure.rs`, `jobs/retention.rs` (M14) | `EraseMessages` after each `EraseRows` batch, `EraseIdentity`, `CloseStreams` and `Erase` in tenant erasure, `Probe`, the receipt fields; `Purge` in the tenant retention job's `events` step |
| `mailbox/erase.rs` (M14), `mailbox/expiry.rs` (M37) | `erased_ids.log_erased_at` and the `log_erase` alarm purpose, which sends `EraseMessages` for what every `EraseRows` deleted, tenant retention and per-identity expiry included (§11.3). If M37 lands first, M30 adds the purpose and runs `it::events::retention_and_expiry_reach_log` against expiry |
| `mcp/tools.rs` (M15) | `mail_events`, with its rows in M15's table test |
| `crates/cli` (M16) | `idvy events …` |
| `console/pages/webhooks.rs` (M21) | The event consumers and streams section |
| `crons/` (M17 and M26's minting loop) | Minting `events_do_id` for rows at `''` |

**Implements:** FR-EVT-1 to FR-EVT-11 and edge rows R1–R69
([Event feed and live stream](design/event-feed.md)).

**Tasks:**

1. **Stream spike first** (design § 7.9): from Rust through `export_worker!`, on workerd and on staging,
   accept with tags, an attachment surviving eviction, the auto-response, the three handlers, a `101` with
   `Sec-WebSocket-Protocol`, 4000-range close codes reaching a native client and a browser, and the deploy
   disconnect. Record the result in the design. If the glue cannot export the handlers, write the one-file
   `entry.rs` ADR before continuing.
2. `core::events`: cursors, the visibility table, filters, index fields, the stream protocol, the window, close
   codes and the token format, each unit-tested.
3. The `TenantEvents` object: schema, `Init`, `Append`, `Read`, `Head`, consumers, tokens, erasure requests,
   `Purge`, `Reset`, `Stats`, and its two alarms.
4. The append step in the `Fanout` consumer, with the retry schedule and the payload reuse.
5. The handlers, the long poll, the stream handshake on both credential paths, revalidation, and the
   push and backstop closes.
6. Erasure, retention and the `event_log_rebuild` job.
7. `mail_events`, the CLI commands and the SDK stream client.
8. The console section, the metrics, the four alerts and the runbook.

**Acceptance:**

- Edge rows R1–R69, with the tests named in the register (`core::events::*`, `it::events::*`,
  `it::stream::*`, `it::mcp::events_tool`, `cli::events::tail_resume_and_exit_codes`,
  `sdk::events::stream_close_rules`, `live::events::stream_through_deploy`).
- The ten routes join `it::security::cross_tenant_matrix`; `sdk::coverage::every_operation` passes with the
  new operations.
- M8's suites (`it::webhooks::*`) pass unchanged, and `it::webhooks::notifier_handoff_never_blocks` reads
  payloads once per batch.
- M14's erasure suites gain the log assertions: `log_events_deleted` and `probe.log_hits = 0` in every scope.
- `it::console::event_consumers_section`; the section joins `browser::console::no_js` and
  `browser::console::axe_scan`.
- NFR-REL-3 still holds on staging with the append in the path (`webhook_delivery_latency_ms`).

---

## M31 · Screening hold (after M7, M8, M12, M14, M15, M16, M17 Foundation, M21 and M26)

**Goal.** Inbound mail can be held until a deterministic scan and a separate classifier have judged whether it
steers agents, with the deadline, release, feedback, summary, events, console, MCP and CLI of
[Screening hold](design/screening.md).

**Why these dependencies.** M7 (the inbound consumer, ingest, release and `wait` it changes), M8 (the outbox
builders and the J29 check for `message.screened`), M12 (the shared model adapter and `core::injection`), M14
(erasure counts, export), M15 (the MCP tool), M16 (the CLI), M17 Foundation (metrics and the alert evaluator),
M21 (the console pages) and M26 (the Notifier hand-off change).

**Files:** `crates/core/src/{screening.rs, visibility.rs}`, `crates/worker/src/screening/{mod.rs, prompt.rs,
schema.rs, summary.rs}`, `consumers/screen.rs`, `mailbox/screening.rs`, `handlers/screening.rs`,
`console/pages/screening.rs`, `crates/cli/src/commands/screening.rs`; the mailbox schema migration of §14. Hooks
in other milestones' files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `core/src/injection.rs` (M2, M10, M12) | `role_marker`, the confusable fold, the added patterns, every pattern compiled through the fold |
| `consumers/inbound.rs` (M7) | Step 11a; `IngestInput.screening`; post-commit step 4a |
| `mailbox/ingest.rs` (M7) | Rule 6a, step 15a, the `received_evented_at` writes, events |
| `mailbox/read.rs` (M7) | Every visibility predicate from `core::visibility`; release: `received_evented_at`, `message.received` (`released: true`), `released_by` on the record; `409 not_quarantined` details |
| `handlers/wait.rs` (M7) | Never return `screening` |
| `mailbox/attachments.rs`, `mailbox/reparse.rs` (M7), `consumers/index.rs` (M7, M10, M12) | The late scan; the embed and triage `Skip` for `screening` |
| `search/*` (M10, M11) | Predicates from `core::visibility`; `include_quarantined` covers `screening`; evidence `steering_suspected` from the record |
| `triage/*` (M12) | `409 triage_not_eligible` reason `screening` |
| `webhooks/payloads.rs`, `handlers/webhooks.rs`, `consumers/webhooks.rs` (M8, M26) | `message_screened` builder; `data.screening`, `released`, `rescreened`; J29 row; the Notifier hand-off without `message.released` |
| `quota/mod.rs` (M5 stub, M12) | `CountScreenModel` with the sender sub-cap (`screen_sender:{h}`); `CountScreenFailure` on the default tenant (`screen_fail`) |
| `deploy/wrangler.toml.tmpl`, `crates/worker/src/lib.rs` (M5) | `Q_SCREEN`, the `pm-screen` consumer and `pm-screen-dlq`. If M5 has already merged, M31 adds them; the rule that one milestone owns each change still holds |
| `jobs/erasure.rs`, `mailbox/erase.rs`, `mailbox/export.rs` (M14) | Receipt counts; export fields |
| `policy/*`, `core/src/policy.rs` (M5, M21) | The three guard fields and their tightening rules |
| `mcp/tools.rs` (M15) | `mail_screening_feedback` and its row in M15's table test |
| `ops/alerts.rs` (M17) | The two conditions |
| `console/pages/{quarantine.rs, thread.rs, policy.rs}` (M21) | §21 |
| `crates/conformance/screening/` | The labelled steering set: at least 400 messages (200 steering across body, quoted history, filenames, attachment text, hidden text and five languages; 200 look-alike legitimate messages), synthetic, on reserved domains |

**Implements:** FR-SCR-1 … FR-SCR-12, edge rows T1–T61, and the cross-tenant suite's four new routes (NFR-SEC-1).

**Acceptance:**

- Every edge row T1–T61 with the tests named in the register.
- The `core::screening::*`, `core::injection::t52_confusable_fold`, `core::policy::t34_screening_guard_fields` and
  `core::visibility::hidden_statuses` tests; `conf::screening::*`.
- `it::screening::t17_hidden_everywhere` passes with every read route of the router table and every MCP tool
  covered.
- With `screening.mode: off`, the M7, M10, M12 and M26 suites pass unchanged; with the default `monitor`, the M7
  suites pass with the screening record added and no status change.
- `it::quarantine::release_transaction` is updated for the new event pair and still passes.
- `live::screening::t1_hold_round_trip` on staging; `eval::screening` records its first baseline in `quality.md`.
- `sdk::coverage::every_operation` passes with the four new operations; the console pages join
  `browser::console::no_js` and `browser::console::axe_scan`.

---

## M32 · Calendar invitations (after M7, M8, M9, M14, M15, M16 and M21)

**Goal:** read, verify and expose inbound iCalendar, send iMIP from caller-supplied objects, and answer
verified invitations, with no calendar store ([Calendar invitations](design/calendar-invitations.md)).

**Files:** `crates/core/src/ical/{mod.rs, lex.rs, value.rs, rrule.rs, tz.rs, windows_zones.rs,
vtimezone.rs, write.rs}`, `crates/core/src/calendar.rs`, `crates/worker/src/mailbox/calendar.rs`,
`handlers/calendar_reply.rs`, `xtask/src/cldr.rs`, `fuzz/fuzz_targets/ical_parse.rs`,
`crates/conformance/corpus/calendar/`, `crates/cli/src/commands/calendar_reply.rs`; no migration (the two
`messages` columns and their index are in mailbox schema v1). Hooks in other milestones' files, each
reviewed by that file's owner:

| File | Hook |
|---|---|
| `consumers/inbound.rs` (M7) | Step 10a (calendar), the step 12 address query, `IngestInput.calendar` |
| `core::classify` (M2, M7) | Row 5 reads the calendar object; step 10a upgrades `normal` to `calendar` |
| `mailbox/ingest.rs`, `core::thread` (M7) | The `UID` step of `resolve_inbound`, `organized_here`, the two columns, `calendar.received` in the outbox; the same event in the release transaction and in `Reparse` |
| `webhooks/payloads.rs`, `handlers/webhooks.rs` (M8) | The `calendar_received` builder and its golden file; `calendar.received` in the J29 permission table |
| `handlers/send.rs`, `core::policy`, `core::compose`, `mailbox/submit.rs`, `transport/cloudflare.rs`, `consumers/outbound.rs` (M9) | The `calendar` field; policy steps 9a, 13a and 13b; recipient rule 5; the iMIP MIME; the stale-sequence re-check in the accept transaction; the raw Cloudflare path or its refusal; no fallback for calendar messages; `forward`'s `application/ics` form; `SendOp::CalendarReply` |
| `jobs/erasure.rs` and the mailbox `ErasurePlan` (M14) | Counterparty matching on `calendar_json.parties`; `calendar` in the export's `messages.json` |
| `mcp/tools.rs` (M15) | `mail_calendar_reply`; `calendar` on `mail_send` and `mail_reply`; the output caps |
| `crates/cli/src/commands/send.rs` (M16) | `--calendar-file`, `--calendar-copy`, `--no-calendar-copy` |
| `console/pages/thread.rs` (M21) | The calendar panel |
| The draft send (M29) and the screening verdict (M31) | Whichever of M29, M31 and M32 lands later adds the `calendar` draft field and the `calendar.received` append at the end of screening |

**Spike:** S1 gains the raw calendar case of [Calendar invitations §5.6](design/calendar-invitations.md#56-transports).
Until its result is recorded, M32 ships with calendar sends on the `cloudflare` transport refused
(`calendar_needs_raw_mime`).

**Implements:** FR-CAL-1 … FR-CAL-14 and edge rows V1–V55, with the rewritten B8.

**Acceptance:**

- Every test of [Calendar invitations §18](design/calendar-invitations.md#18-tests) passes, including the
  corpus cases `conf::calendar::v*` (native and through workerd) and 60 seconds of `ical_parse` in
  `fuzz-smoke` without a crash.
- The existing suites pass unchanged: `conf::mime::b8_ics` is replaced by `conf::calendar::v1_duplicate_parts`;
  `it::send::*`, `it::inbound::*` and `it::thread::*` pass with the new steps.
- `POST …/calendar-reply` joins `it::security::cross_tenant_matrix`, and `sdk::coverage::every_operation`
  passes with the new operation.
- The console panel joins `browser::console::no_js` and `browser::console::axe_scan`.
- At M20, `live::calendar::inbound_real_invitations`, `live::calendar::client_matrix` and
  `live::calendar::reply_applied` pass, and their results for the `invite.ics` copy and the generated
  `VTIMEZONE` are recorded on the design page.

---

## M33 · Uploads (after M7, M9, M14, M15, M16 and M22)


**Files:** `crates/core/src/uploads.rs` (create validation, the state machine, the upload-token payload,
expiry arithmetic; pure), `crates/worker/src/uploads/{mod.rs, put.rs, classify.rs, copy.rs}`,
`handlers/uploads.rs` (the four `…/uploads` routes), `mailbox/uploads.rs` (the `uploads` table, its
operations and `alarm:uploads`), `crates/cli/src/commands/uploads.rs`. Mailbox schema: the `uploads` table
and `attachments.upload_id`, in mailbox schema v1 while nothing is built
([Data model](design/data-model.md)), as for M29. Hooks in other milestones' files, each reviewed by that
file's owner:

| File | Hook |
|---|---|
| `crates/platform/src/lib.rs`, `cf/glue.rs` (M4) | `BlobStore::put_stream(key, BodyStream, len, sha256, meta)`, `head_checksum(key) -> Option<(u64, Option<[u8; 32]>)>` and `copy(from, to, len, sha256)`; `RequestBody::stream()` for the handler; the fakes gain the same, with fault injection for `10037`, `10013` and `10058` |
| `router.rs`, `auth/routespec.rs` (M5) | `RouteSpec.any_permissions`; the four routes and `PUT /v1/links/{token}` |
| `handlers/links.rs` (M9) | `PUT` with purpose `upload`; `GET` refuses `upload` tokens |
| `mailbox/submit.rs`, `core/src/compose.rs` (M9) | Policy step 12 additions, the size plan from sizes, steps 4, 6 and 7 with uploads, pins; classification of inline base64 items |
| `core/src/attach.rs` (M7) | `classify_name` exposed for the final filename |
| `jobs/erasure.rs`, `jobs/backup.rs`, `mailbox/maintenance.rs` (M14) | `ErasurePlan` and `EraseRows` with source uploads, `UploadPlan` and `EraseUploads`, `uploads_deleted`; the backup job skips `up/`; the orphan sweep names `uploads.r2_key`; `Hygiene` runs the uploads sweep |
| `billing/storage.rs` (M22) | Upload bytes in the mailbox storage report; the create gate |
| `mcp/tools.rs` (M15) | `mail_create_upload`, `mail_get_upload`, both item shapes in the send tools, `attachments` on `mail_forward`, with rows in M15's table test |
| `crates/cli/src/commands/send.rs` (M16) | `--attach` through uploads above 1 MiB |
| `observability/catalogue.rs`, alert table (M17) | The nine metrics and `upload_object_missing` |
| Drafts (M29) | Whichever of M29 and M33 lands second adds the draft-save copy (`uploads::copy_to`, with the pin and the `draft_att` marker), `drafts:write` in the upload routes' `any_permissions` (M29 defines the permission), and `it::uploads::q108_draft_reference` |

**Implements:** FR-UPL-1 to FR-UPL-12 and edge rows Q65–Q108 ([Uploads](design/uploads.md)).

**Order.** `it::uploads::v_put_stream_contract` is the first pull request of the milestone: it runs the
`FixedLengthStream` and `sha256` behaviour against workerd's R2 and settles §3.2 steps 7 and 8 before any
handler code.

**Acceptance:**

- Edge rows Q65–Q108 with the tests named in the register, all passing.
- `core::uploads::*`, `core::attach::v19_final_name_reclassified`, `core::compose::v23_link_plan_from_sizes`.
- `it::uploads::scope_matrix`; the routes join `it::security::cross_tenant_matrix`.
- `it::uploads::erasure_scopes` fills the uploads rows of M14's erasure suites; the receipt schema has
  `uploads_deleted`.
- `it::uploads::lifecycle_and_reads`, `it::mcp::upload_tools`, `cli::uploads::create_and_attach`;
  `sdk::coverage::every_operation` passes with the four new operations and the `PUT`.
- `live::uploads::large_attachment_link` passes in M20, and its `upload_copy_ms` for 20 MiB is below the
  submit deadline with at least a factor of 3 to spare; otherwise the per-message attachment total is
  lowered before release.
- With no upload used, the M9 send suites pass unchanged, except `it::uploads::inline_base64_classified`'s
  deliberate change (risky inline attachments are refused).

---

## M34 · Agent self-sign-up (after M24, M26, M27, M29, M30, M31 and M33)

**Goal:** an agent with no key creates a provisional Free workspace with one inbox and one key, which
can email only the person it names until that person verifies; unverified workspaces are erased after 7
days. Behind `PM_AGENT_SIGNUP` (off), on Pylota Identity Cloud only ([Agent self-sign-up](design/agent-signup.md)).

**Files:** `crates/core/src/signup.rs` (pure: normalisation, the username and human-email rules, the base
address, the permission and policy overlays, code and token encodings); `crates/worker/src/signup/{mod.rs,
create.rs, admit.rs, verify.rs, human.rs, end.rs, mail.rs, outcomes.rs}`; `handlers/agent_signup.rs`;
`console/pages/agent_signup.rs`; `crons/agent_signup.rs` (on the `*/15` cron); no migration (the two
tables, `tenants.provisional_until`, `pending_auth.agent_signup_id` and the `ops_switches` value are in
`0001_init.sql`). Hooks in other milestones' files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `auth/mod.rs` (M5) | Step 9: `provisional_until` read with the tenant row, the permission overlay |
| `auth/routes.rs` (M5) | `Scope::AgentSignup` and its entry in the empty-permission exception list |
| `crates/core/src/policy.rs` (M2, M5) | `effective(…, provisional)` with `PROVISIONAL_POLICY`; send steps 4 and 7a |
| `handlers/send.rs` (M9) | The human-address read for a provisional tenant |
| `consumers/inbound.rs` (M7) | Passes `provisional_until` to `effective` |
| `consumers/delivery.rs` (M9) | Sign-up outcomes, `CountSignupOutcome`, the `agent_signup_off` rule |
| `quota/mod.rs` (M5 stub, M24 `SystemMail`) | `SystemMailClass::Signup` and its budgets; `CountSignupOutcome` |
| `handlers/identities.rs`, `handlers/addresses.rs` (M6) | `409 address_taken` at a live sign-up's human address |
| `console/pending.rs` (M24) | The `two_factor` step with `agent_signup_id` |
| `console/pages/settings.rs` (M24) | The refusal notice and **Allow again** |
| `crons/signup_ramp.rs` (M24) | `verified_at` as the ramp's start for verified agent workspaces |
| `notify/` (M26) | `AccountEvent::AgentSignupVerified` |
| `jobs/erasure.rs` (M14) | `agent_signups` in `delete_d1_rows` and in the step-8 probe; person deletion's `human_hash` scrub |
| `handlers/uploads.rs` (M33) | `403 permission_denied` (`signup_unverified`) while provisional |
| `mcp/tools.rs` (M15) | The four `mail_signup_*` tools and their rows in M15's table test |
| `crates/cli/src/commands/agent_signup.rs` (M16) | `idvy agent-signup …`, `idvy agent-signups list` |
| `crons/alerts.rs` (M17) | `agent_signup_spike`, `agent_signup_cap_80`, `containment:agent_signup_off` |

**Implements:** FR-ASU-1 to FR-ASU-16 and the amended FR-IDN-2; edge rows X1–X59 and X61–X63 (X60 is
integrator-owned).

**Acceptance:**

- Every test of [Agent self-sign-up § 25](design/agent-signup.md#25-tests): the `core::signup::*` and
  `core::policy::x21_provisional_send_steps` tests; the `it::signup::*` tests; `it::mcp::signup_tools`;
  `it::console::agent_signup_pages` (the pages join `browser::console::no_js` and
  `browser::console::axe_scan`); `cli::agent_signup::create_writes_profile`.
- The new routes join `it::security::cross_tenant_matrix`; `sdk::coverage::every_operation` passes with
  the seven new operations; `it::logs::i5_no_content_in_logs` carries the new canaries;
  `platform::config::startup_rules` covers the two variables.
- With `PM_AGENT_SIGNUP = "off"` every M24, M26 and M27 suite passes unchanged.
- `live::signup::x40_refuse_one_click_headers` and `live::signup::end_to_end` pass on staging (M20's
  environment) before the gate below.

**Gate:** `PM_AGENT_SIGNUP` stays `off` on Cloud until gate D of [Cloud commissioning](cloud-commissioning.md#10-go-live-gates):
phase C has run for at least 30 days with no `containment:*` page, no `shared_domain_breaker` at stage 2
and no unresolved `signup_ramp_review`; DMARC on the platform domain is at `p=reject`; the two `live::signup`
tests pass on production with the variable on for one hour, then it is turned off again until the owner
decides.

---

## M35 · Deliverability metrics (after M9, M13, M14, M15, M16 and M21)


**Files:** `crates/core/src/abuse.rs` (the window catalogue, `evaluate`, `decide`, `headroom`, parts per
million; pure), `crates/core/src/metrics.rs`, `crates/worker/src/metrics/{mod.rs, rates.rs, events.rs}`,
`handlers/metrics.rs`, `quota/rates.rs`, `mailbox/metric_counts.rs`, `console/pages/deliverability.rs`,
`crates/cli/src/commands/metrics.rs`. Mailbox schema: `metric_counts`, in mailbox schema v1 while nothing is built.
Hooks in other milestones' files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `quota/outcomes.rs`, `quota/mod.rs` (M9, the M5 stub) | `RecordOutcome.thresholds`; the evaluation moved behind `core::abuse::{window_states, evaluate, decide}` with the pinned windows; `GetRates` |
| `consumers/delivery.rs` (M9) | Resolve `AbuseThresholds` from the effective policy (60-second isolate cache) and pass them |
| `mailbox/outbox.rs` (M6) | The `metric_counts` upserts beside each counted outbox append |
| `mailbox/maintenance.rs` (M6) | Pruning of `metric_counts` |
| `router.rs`, `auth/permissions.rs` (M5) | `metrics:read`; the two routes |
| `mcp/tools.rs` (M15) | The two tools, with rows in M15's table test |
| `console/pages/{inboxes.rs, domains.rs}` (M21) | The identity and domain windows |
| `observability/catalogue.rs` (M17) | The four metrics |

**Implements:** FR-MET-1 to FR-MET-10 and edge rows Y1–Y19 ([Deliverability metrics](design/metrics.md)).

**Order.** The first pull request moves the existing pause evaluation behind `core::abuse` with no
behaviour change except the pinned windows and the thresholds argument; the M9 delivery suites
(`it::delivery::*`, G12, G13, J33) must pass unchanged on it before the routes are added.

**Acceptance:**

- Edge rows Y1–Y19 with the tests named in the register, all passing.
- `core::abuse::y_rates_match_decision` and `core::abuse::headroom_properties`, each with at least 10,000
  generated cases per run.
- `it::metrics::events_counts_match_outbox`, `it::metrics::record_outcome_thresholds`.
- `it::metrics::scope_matrix`; the routes join `it::security::cross_tenant_matrix`.
- `it::console::deliverability_page`; the page joins `browser::console::no_js` and `browser::console::axe_scan`.
- `it::mcp::metrics_tools`, `cli::metrics::rates_and_events`; `sdk::coverage::every_operation` passes
  with the two new operations.
- `live::metrics::bounce_moves_rates` passes in M20.

---

## M36 · SDKs, skills and agent tooling (after M8, M15, M16 and M19; the event, draft and upload helpers after M30, M29 and M33)

**Goal.** Integrators and coding agents get a TypeScript SDK, skills and MCP and CLI tooling that keep the
service's safety guarantees on the client side, so no integration hand-writes retries, keys, webhook
verification or stream resume.

**Files:** in this repository, `sdks/fixtures/**`, `sdks/typescript-tests.json`, `integrations/**`,
`.claude-plugin/marketplace.json`, `.agents/plugins/marketplace.json`, `.cursor-plugin/marketplace.json`,
`xtask/src/{gen_skills.rs, skills_check.rs}`, `crates/cli/src/{schema.rs, query.rs, commands/skills.rs,
commands/mcp_stdio.rs}`, the new guides. In the SDK repository `PILOTAAI/pylota-identity-typescript`
([ADR 0018](adr/0018-typescript-sdk-in-its-own-repository.md)): the package, its generator, tests, compiled guide examples and
release workflows ([SDKs, skills and agent tooling § 2.1](design/sdk-and-skills.md#21-package-and-layout)). Hooks in other
milestones' files, each reviewed by that file's owner:

| File | Hook |
|---|---|
| `migrations/d1/0001_init.sql` (M5) | `api_keys.mcp_tools_json` |
| `crates/worker/src/auth.rs`, `handlers/keys.rs` (M5) | `Scope.mcp_tools`; minting validation, audit, `GET /v1/me` |
| `crates/worker/src/mcp/{tools.rs, schemas.rs}` (M15) | Effect classes, computed annotations, `_meta`, the allow-list filter, description budgets |
| `crates/sdk` (M16) | `Unknown(String)` for response enums and `ErrorCode`; fixtures and their canonical decodings written to `sdks/fixtures/` |
| `crates/cli/src/commands/mcp.rs` (M16) | `mcp config --client codex\|claude-desktop\|agent-sdk` |
| `console/pages/keys.rs`, `console/pages/connect.rs` (M21) | The MCP tools fieldset; the new snippets |
| `.github/workflows/ci.yml` (M19) | Job `skills` |
| `xtask` `release` (M19), `itest` (M5), `trace` (M0) | `openapi.json`, `sdk-fixtures-{version}.tar.gz` and `ts-tests-required.json` as release assets listed in `SHA256SUMS`; `release sign --repo PILOTAAI/pylota-identity-typescript` (`xtask::release_sign_sdk_repo`); `itest --serve`; `trace` reads `ts::` names from `sdks/typescript-tests.json` |

**Tasks.**

1. In this repository: the shared fixtures, the two release assets, `itest --serve` and the `trace`
   inventory. In the SDK repository: the generator, the generated layer and the hand-written layer of
   §2.4–§2.9 and §2.13–§2.14.
2. Webhook verification and the handler (§2.10).
3. `mcp_tools` end to end: column, minting rules, filter, `GET /v1/me`, console fieldset.
4. Effect classes, `_meta` hints and description budgets in the MCP server.
5. `idvy mcp stdio`, `mcp config` clients, `--schema`, `--query`, `skills generate`.
6. The skills, the rule registry, `gen-skills`, the checks and the three marketplaces.
7. The SDK repository's release path: `npm pack` and the owner's signature in its `release.yml`, trusted
   publishing in its `release-publish.yml`, the contract and version checks, the npm trusted-publisher
   configuration (a human step on npmjs.com).
8. After M30: the event helpers of §2.11. After M29 and M33: the draft and upload helpers of §2.12.
9. The guides of §7.

**Human prerequisites.** Create the public repository `PILOTAAI/pylota-identity-typescript` (FSL-1.1-ALv2).
Confirm that TREFT LTD owns the npm organisation `pylotahq`; on npmjs.com, configure the trusted publisher
for `@pylotahq/identity` (repository `PILOTAAI/pylota-identity-typescript`, workflow `release-publish.yml`,
environment `release`) and turn on "Require two-factor authentication and disallow tokens" after the first
publish. Give the SDK repository a staging test key for `ts::live::round_trip`. Pin Deno and Bun
versions for the SDK repository's CI.

**Acceptance:**

- Edge rows Y20–Y71 with the tests named in the register; the `ts::` tests pass in the SDK repository's
  CI and are listed in `sdks/typescript-tests.json`; the helpers of rows Y32–Y37 accept on M30, Y47 on M29
  and Y46 on M33, and are `pending` in `cargo xtask trace` until those land.
- The first release after M36 carries `openapi.json` and `sdk-fixtures-{version}.tar.gz` in its signed
  `SHA256SUMS`, and the SDK repository's `fetch-contract.mjs` verifies and uses them.
- `ts::coverage::every_operation`, `ts::generate::output_current`, `ts::fixtures::rust_parity`,
  `ts::errors::envelope_round_trip`, `ts::package::exports_valid`, `ts::package::no_install_scripts`,
  `ts::compat::pylota_tsconfig`, `ts::compat::dts_consumers`, `ts::doctest::published_examples`,
  `ts::runtime::node_matrix`, `ts::runtime::deno_bun_smoke`, `ts::runtime::y66_workers_smoke` and
  `ts::retry::unsafe_request_not_repeated`.
- `it::mcp::annotations_complete`, `it::mcp::y53_result_size_meta`, `it::mcp::y54_description_budgets`,
  and the extended `it::security::mcp_tools_follow_key`; `it::mcp::inspector_replay` still replays green.
- `cli::skills::generate_for_key` and `cli::mcp::y59_client_snippets`; the `skills` job green with
  `skills-ref validate` (and `claude plugin validate --strict` if it runs without signing in).
- The SDK repository's first release candidate produces `pylotahq-identity-{version}.tgz` in its draft
  release, listed in its owner-signed `SHA256SUMS`; a full release publishes it to npm with provenance, and
  `npm audit signatures` in a fresh project reports a verified attestation for it.
- `ts::doctest::framework_guides` green; `eval::skills_safe_send` has a recorded first pass rate.
- M20 gains `ts::live::round_trip`.

---

## M37 · Parity additions (after M7 to M16, M21, M23 and M27)

**Goal:** the nine additions of [Parity additions](design/parity-additions.md) (section numbers below are
that page's), each wired end to end, with the register's section Z green. The nine are independent of each other and can land as separate pull requests in any order, each with its own
rows of section Z.

**Tasks**, in a workable order:

1. Docs link (§10): `docs`, `ErrorCode::anchor()`, `PM_DOCS_URL`, the anchors in `errors.md`, `cargo xtask
   docs-check`. First, because every later error code needs its anchor.
2. Trust detail (§5): `spf.aligned`, `arc.{sealer, sealer_trusted, aar}` in `auth_json`; the seven `trust` fields.
3. Trusted ARC sealers (§6): the policy field, `core::auth::arc_rescue`, the step-12 binding read, the verdict
   value, each consumer of the §6.5 table; the live test on staging.
4. Batch operations (§2): the two routes, `BatchGet` and `BatchUpdate`, the audit rows, MCP and CLI.
5. Label events (§4), on top of 4: the policy field, `OPT_IN_EVENT_TYPES`, the outbox append in the three
   label-changing requests.
6. Identity-scope lists (§3): the table change, `core::lists::decide`, the four routes, the inbound and
   outbound readers, erasure counts.
7. Webhook headers (§8): the columns and sealed entry, create and `PATCH …/headers`, delivery, host-change
   clearing.
8. Retention (§7): the identity field, the `set_retention` intent, `mailbox/expiry.rs`, the idempotency stub,
   the delivery stub (§7.13), the daily receipt, the alert.
9. Zone export (§9): `core::zonefile::render` and the `format` parameter.

**Files:** as the Code row of [Parity additions](design/parity-additions.md). One migration, `migrations/d1/00NN_parity_additions.sql` only if
`0001_init.sql` has shipped to a production deployment when M37 lands; otherwise `0001_init.sql` and mailbox
schema v1 are edited in place (`sender_lists`, `identities.message_hours`, the three `webhook_endpoints`
columns, the `rpc_intents.op` value and the `messages.verdict` value). Hooks in other milestones' files, each
reviewed by that file's owner:

| File | Hook |
|---|---|
| `mailbox/read.rs` (M7) | `BatchGet`, `BatchUpdate`, and the label-event append in `UpdateMessage` and `UpdateThread` |
| `consumers/inbound.rs`, `mailbox/ingest.rs` (M7) | Identity-scope list rows at step 12; the ARC rescue in steps 7 and 12; `arc_rescued` in the trust signals, caps and verification rules |
| `crates/core/src/auth.rs`, `trust.rs` (M2, M7) | `arc_rescue`, the trust detail fields, `auth_json` keys |
| `handlers/wait.rs` (M7), `accounts` gate (M27) | `arc_rescued` is not `pass` for releases and the ledger match |
| `handlers/send.rs`, `consumers/outbound.rs`, `mailbox/idempotency.rs` (M9) | Identity-scope send entries at step 6 and composition step 1; the expired stub in the reservation |
| `consumers/delivery.rs`, `ApplyDeliveryEvent` in the mailbox (M9) | The `delivery_stubs` lookup after reconciliation, its transitions and `metric_counts`, and the answer with the stub's `first_reply` and `domain_id` (§7.13) |
| `webhooks/payloads.rs`, `consumers/webhooks.rs`, `handlers/webhooks.rs`, `webhooks/client.rs` (M8) | The `labels_changed` builder, `OPT_IN_EVENT_TYPES`, headers on delivery, `PATCH …/headers`, host-change clearing |
| `search/citations.rs` (M11), `triage/rules.rs` (M12) | The `Rescued` evidence state; rule 13 |
| `handlers/domains.rs` (M13, M23) | `format=bind` |
| `mailbox/erase.rs`, `jobs/erasure.rs` (M14) | `EraseRows { mode: Expiry }` and `{ mode: Retention }` (delivery stubs only); `list_entries_deleted`; the identity-scope list rows of identity erasure |
| `handlers/identities.rs`, `crons/intents.rs` (M6) | `retention`, the `set_retention` intent |
| `crates/api-types/src/errors.rs` (M3), `crates/worker/src/errors.rs` (M5) | `docs`, `ErrorCode::anchor()`, `PM_DOCS_URL` |
| `mcp/tools.rs` (M15) | `mail_update_labels` with `message_ids` and `require_absent` |
| `crates/cli/src/commands/*` (M16) | The commands of the page |
| `console/pages/*` (M21) | Inbox retention and lists, ARC wording, webhook header names, zone download |
| `crates/core/src/sealed.rs` (M17 Foundation) | `webhook_endpoints.headers_sealed`, with its case in `it::secrets::master_key_rotation` |
| `drafts` (M29), event feed (M30) | Draft expiry and `beyond_retention` (M29); feed entries of label events and of expired messages through the mailbox's `log_erase` purpose (M30, R65 and R66). If M29 or M30 lands after M37, that milestone adds its hook and runs the test named for it (`it::drafts::z61_drafts_expire`, `it::events::retention_and_expiry_reach_log`, `it::events::label_event_erasure`) |

**Implements:** FR-IN-13, FR-IN-14, FR-IN-15, FR-OUT-17, FR-WH-6, FR-WH-7, FR-PRV-7, FR-DOM-15, FR-API-3, and edge
rows Z1–Z81.

**Acceptance:**

- Every test of [Parity additions § 15](design/parity-additions.md#15-tests), by row: Z1–Z12 (batch), Z13–Z23
  (lists), Z24–Z29 and Z80 (label events), Z30–Z32 (trust detail), Z33–Z46 (ARC), Z47–Z63 and Z81 (retention), Z64–Z71 (webhook
  headers), Z72–Z76 (zone export), Z77–Z79 (docs links).
- The new routes (seven operations) and the changed ones join `it::security::cross_tenant_matrix`;
  `sdk::coverage::every_operation` passes with the new operations; `it::mcp::input_schemas_plain_objects` passes.
- `it::webhooks::filters` asserts that `*` includes every type except `message.labels_changed`.
- `live::arc::cloudflare_forward_behaviour` has run on staging and its result is recorded in
  [Parity additions § 6.1](design/parity-additions.md#61-sources-read-on-2026-10-17) as a "Spike result" note, on the first staging candidate that
  carries M37 ([Testing § 10](design/testing.md#10-live-end-to-end-suite-live)); `live::docs::z78_anchor_resolves`
  runs in the same suite.
- `cargo xtask docs-check` is in the CI gate and passes; `mdbook build docs` passes.
- With every new setting at its default (`events.labels_changed: false`, no identity lists,
  `retention.message_hours: null`, no webhook headers), the M7, M8, M9 and M14 suites pass unchanged, except
  the trust golden files, which gain the seven fields, and `it::webhooks::filters`.

---

## M38 · Phone numbers, requests and compliance (after M5, M6, M8, M14, M17 Foundation, M21, M22 and M27)

M5 for the router, `key_may_decide` and the `TenantQuota` stubs; M6 for identities; M8 for platform events and
fan-out; M14 for the erasure and retention jobs; M17 Foundation for metrics and alerts; M21 for the console;
M22 for holds and Stripe; M27 for the service ledger that [Phone numbers § 11.2](design/phone-numbers.md#112-the-gate) extends.

**Files:** `crates/core/src/phone/{mod.rs, e164.rs, catalogue.rs, requests.rs, budget.rs, onboarding.rs,
capabilities.rs, numbers.rs, orders.rs, money.rs, telnyx.rs}`, `crates/core/data/{phone-catalogue.json,
phone-rate-card.json}`, `crates/worker/src/phone/{provider.rs, telnyx/*, fake.rs, execute.rs, buy.rs,
onboarding.rs, orders.rs, numbers.rs, tenant_resources.rs, compliance.rs, documents.rs, money.rs}`,
`handlers/phone/{catalogue.rs, setup.rs, budget.rs, buy.rs, numbers.rs, requests.rs, compliance.rs, charges.rs,
platform.rs}`,
`handlers/hooks_telnyx.rs`, `consumers/phone.rs` (kinds `provider_event`, `execute`, `reconcile`, `configure`),
`crons/phone.rs`, `console/pages/phone/{setup.rs, budget.rs, numbers.rs, requests.rs, compliance.rs, charges.rs}`,
`crates/cli/src/commands/phone.rs` (setup, countries, setup status and drafts, budget, buy, search, request,
requests, numbers, compliance, documents, charges). D1 tables of [Data model](design/data-model.md) for numbers, releases, requests, profiles, documents,
requirement groups, tenants, provider events and charges.

| File (owner milestone) | Hook |
|---|---|
| `deploy/wrangler.toml.tmpl`, `lib.rs` (M5) | The binding `Q_PHONE`, queues `pm-phone` and `pm-phone-dlq`, the R2 lifecycle rule for `phone-staging/`; an exception to "no milestone adds a binding", as M31's `Q_SCREEN` is |
| `quota/mod.rs` (M5, M22) | `QuotaRequest::PhoneSpendHold`, `PhoneSpendSettle`, `PhoneSpendRelease` (the hold taking the budget slot `phone_budget:t:*` with the spend in one transaction) and the counters `phone_spend:*`, `phone_spend:reg:*`, `phone_budget:t:*`, `phone_search`; an exception to "no new variants", as M35's is |
| `auth/people.rs` (M5) | `Decision::ApprovePhoneRequest`, `Decision::DecidePortOut`, `Decision::SetPhoneBudget` |
| `auth/routespec.rs`, `router.rs` (M5) | The phone routes and the seven permissions |
| `core/src/sealed.rs` (M17) | The sealed columns of [Phone numbers § 23](design/phone-numbers.md#23-security-t2) |
| `jobs/erasure.rs`, `jobs/retention.rs` (M14) | `phone_release`, the document steps, receipt fields; retention of requests, events, index and charges |
| `billing/*` (M22) | The `phone_spend` hold feature |
| `handlers/identities.rs` (M6) | `409 identity_has_phone_numbers` |
| `observability/catalogue.rs` (M17) | The metrics and alerts of [Phone numbers § 29](design/phone-numbers.md#29-observability) |
| `mcp/tools.rs` (M15) | `phone_list_numbers`, `phone_search_numbers`, `phone_buy_number`, `phone_setup_status`, `phone_get_request` |

**Implements:** FR-PHN-1 to FR-PHN-20, FR-PHN-22 (order events), FR-PHN-39 (number charges and caps), FR-PHN-40,
FR-PHN-41 (its tools), FR-PHN-43, FR-PHN-44, FR-PHN-45 (the facets that need no registration), FR-PHN-46,
FR-PHN-47, FR-PHN-49 (GB onboarding and the status route), FR-PHN-50, FR-PHN-51, FR-PHN-53, and edge rows P1–P41,
P73–P76, P98, P99, P101, P103, P104, P108–P114, P117, P118, P120–P123, P127, P129–P131.

**Order.** The first pull request is `live::phone::order_contract` and `live::phone::webhook_signature` with the
pure mapping tables, so the status tables rest on the real API before any state machine is built.

**Acceptance:** the rows above with their tests; `it::phone::scope_matrix` joins
`it::security::cross_tenant_matrix`; `sdk::coverage::every_operation` passes with the new operations.

---

## M39 · SMS (after M38, M30 and M31)

M30 for event-feed visibility; M31 for the screening job SMS shares.

**Files:** `crates/core/src/phone/{sms.rs, segments.rs, consent.rs, verify.rs, a2p.rs}`, `crates/worker/src/phone/{sms_out.rs,
sms_in.rs, a2p.rs, codes.rs}`, `handlers/phone/{sms.rs, wait.rs, consent.rs}`, `mailbox/sms.rs` (the SMS tables,
`IngestSms`, `SubmitSms`, `BeginSmsTransport`, `RegisterSmsWait`, `VaultTakeSmsCode`), the `send_sms` kind of
`consumers/phone.rs`, `console/pages/phone/consent.rs`, CLI `sms`, `consent`, `registrations`.

| File (owner milestone) | Hook |
|---|---|
| `consumers/screen.rs` (M31) | `ScreenJob::Sms` |
| `handlers/accounts.rs`, `core/src/accounts.rs` (M27) | `phone_number_id` and `sms_senders` on ledger entries |
| `vault/*` (vault milestone M44) | `SmsCodeSource` and rule 2b land with whichever of M39 and M44 comes second, with `it::phone::vault_code_withheld` and `it::vault::sms_otp_phase` |
| `mcp/tools.rs` (M15) | `phone_send_sms`, `phone_list_sms`, `phone_get_sms`, `phone_wait_sms`, `phone_record_opt_out` |

**Implements:** FR-PHN-21 to FR-PHN-31, FR-PHN-45 (`outbound_sms` with registrations), FR-PHN-48, FR-PHN-49 (US
onboarding, drafts for 10DLC), FR-PHN-52 (texting from US numbers and cross-border destinations), FR-PHN-54 (the
sole-proprietor brand, where the decision allows it), and edge rows P42–P72, P77 (SMS charges in the job's input),
P80–P84, P105, P106, P115, P116, P119, P124 (SMS half), P125, P128.

**Acceptance:** the rows above; `live::phone::send_idempotency` decides whether the repeats of [Phone numbers § 12.3](design/phone-numbers.md#123-dispatch-provider-idempotency-and-uncertain) are enabled;
`live::phone::redaction_levels` recorded in [Phone numbers § 8](design/phone-numbers.md#8-tenant-provider-resources); `live::phone::a2p_paths`, `live::phone::sole_prop_otp`,
`live::phone::cross_border_sms` and `live::phone::us_onboarding_end_to_end` pass on staging (the last gates step 5
of [§2.5](design/phone-numbers.md#25-us-enablement), and its measured waits replace the catalogue's `typical_wait_days`);
`live::phone::uk_brand_registration` is run and its result recorded, whatever it is.

---

## M40 · Voice routes and calls (after M38)

**Files:** `crates/core/src/phone/voice.rs`, `crates/worker/src/phone/{voice.rs, vapi.rs}`,
`handlers/phone/{routes.rs, connectors.rs, calls.rs}`, `mailbox/calls.rs`, CLI `connectors`, `route`, `calls`,
`usage`; MCP `phone_place_call`, `phone_list_calls`.

**Implements:** FR-PHN-32 to FR-PHN-35, FR-PHN-52 (calls from US numbers for tenants registered elsewhere) and
edge rows P85–P91, P124 (calls half).

**Acceptance:** the rows above; `live::phone::vapi_route`, `live::phone::vapi_call_list`,
`live::phone::forwarding` and `live::phone::e911_address` pass on staging.

---

## M41 · Porting, port-out guard, reconciliation, invoicing and migration (after M38, M39, M40 and the vault core M42)

**Files:** `crates/core/src/phone/porting.rs`, `crates/worker/src/phone/{porting.rs, port_out.rs, reconcile.rs,
invoicing.rs}`, `handlers/phone/port_outs.rs`, `jobs/phone_reconcile.rs`, `console/pages/phone/port_outs.rs`,
the adopt route, CLI `port-outs`, `reconcile`, `statement`, `numbers adopt`.

**Implements:** FR-PHN-36 to FR-PHN-38, FR-PHN-39 (invoicing and statements), FR-PHN-42, FR-PHN-54 (the recorded
decision and the legacy exit path), and edge rows P77–P79, P92–P97, P100, P102, P126. A backfill step sets a vault
PIN on every number activated before this milestone.

**Acceptance:** the rows above; a staging rehearsal of Pylota's migration (adopt or port a test number, send and
receive, place an approved call, release); `live::phone::stripe_invoice_item`; seven days of the telemetry of
[Phone numbers § 30](design/phone-numbers.md#30-rollout-telemetry-and-rollback) on staging before `PM_PHONE` is turned on in production; the sole-trader decision recorded in ADR 0020;
no adopted number left without a lawful path (each legacy number has a `legacy_deadline` or a recorded
re-papering).

---

## M42 · Vault core: keys, items, audit and erasure (after M5, M6, M14, M17 Foundation and M27)

M5 for the router, permissions and `key_may_decide`; M6 for identities; M14 for the erasure and retention jobs;
M17 Foundation for metrics and alerts; M27 for the service ledger that every item is bound to.

**Files:** `crates/core/src/vault/{mod.rs, origin.rs, item.rs, refuse.rs, cookies.rs, audit.rs}`,
`crates/core/src/crypto/{hpke.rs, pv1.rs}`, `crates/api-types/src/vault.rs`, `crates/worker/src/vault/{mod.rs,
keys.rs, items.rs, audit.rs, erase.rs, channels.rs, managed.rs}`, `handlers/vault.rs` (items, tenant status,
audit, custody, rotate-key), `console/pages/vault.rs` (items and audit), `crates/cli/src/commands/vault.rs`
(`status`, `items`, `audit`, `custody`, `rotate-key`), `xtask` checks `vault_types_have_no_debug` and
`docs_forbidden_claims`. D1 tables `vault_keys`, `vault_custody_slots`, `vault_items`, `vault_item_versions`,
`vault_audit`, `vault_audit_anchors`; the column `service_accounts.login_domains_json`; R2 prefix
`t/{tenant_id}/vault/`.

| File (owner milestone) | Hook |
|---|---|
| `auth/routespec.rs`, `router.rs` (M5) | The item, status, audit and custody routes; the six permissions and their level rules |
| `core/src/sealed.rs` (M17) | `vault_keys.private_sealed`, `vault_custody_slots.private_wrapped` |
| `jobs/erasure.rs`, `jobs/retention.rs` (M14) | The vault steps and receipt fields |
| `handlers/accounts.rs`, `core/src/accounts.rs` (M27) | `login_domains`, `vault_login`; closing an entry archives its items |
| `observability/catalogue.rs` (M17) | `vault_audit_append_failures_total`, `vault_decrypt_failed`, `vault_unlock_failures_total` |
| `crates/worker/src/phone/*` (M41) | `put_managed` and `delete_managed` for the port-out PIN |

**Order.** The first pull request is spike S-VLT1 with `core::crypto::hpke::rfc9180_vectors` and
`live::vault::hpke_wasm`; the second is S-VLT2 with `live::vault::d1_audit_contention`. No other vault code merges
before both are recorded in [Design index › Spikes](design/index.md#spikes).

**Implements:** FR-VLT-1–7, FR-VLT-21, FR-VLT-22, FR-VLT-24, FR-VLT-25 and edge rows U1–U21, U59–U63, U66–U69, U83,
U85, U90.

**Acceptance:** the rows above with their tests; `xtask::vault_types_have_no_debug` and
`xtask::docs_forbidden_claims` run in CI; `it::security::cross_tenant_matrix` covers the new routes.

---

## M43 · Grants, uses, approval, consumers and tickets (after M42, M15, M16, M21 and M25)

M15 for the MCP tools; M16 for the CLI; M21 for the console's approval pages; M25 for the RFC 9421 signature base
the consumer routes share with Web Bot Auth.

**Files:** `crates/core/src/vault/{policy.rs, digest.rs, ticket.rs, consumer.rs}`, `crates/worker/src/vault/{grants.rs,
uses.rs, tickets.rs, consumers.rs, redeem.rs, status.rs}`, `handlers/vault_consumer.rs` (`self`, `redeem`, `report`,
`write-back`, `uses/{use_id}`), `auth/consumer.rs`, `console/pages/vault_approvals.rs`,
`console/pages/vault_consumers.rs`, `crates/conformance/vault/` (fixture server, fixtures and a reference
consumer), CLI `grants`, `uses`, `consumers`, `consumer-keys`, `consumer-check`, `conformance`. D1 tables
`vault_grants`, `vault_uses`, `vault_consumers`, `vault_consumer_nonces`.

| File (owner milestone) | Hook |
|---|---|
| `auth/people.rs` (M5) | `Decision::ApproveVault`, `Decision::ActivateConsumer` |
| `auth/routespec.rs` (M5) | `Auth::ConsumerSignature` on the five consumer routes |
| `mcp/tools.rs` (M15) | `vault_list_items`, `vault_get_item`, `vault_request_grant`, `vault_list_grants`, `vault_get_use`; `vault_login` on `mail_request_account` |
| `core/src/httpsig.rs` (M25) | Reused for consumer signature bases; no change |
| `notify/*` (M26) | Vault approvals in "Needs a person" and the daily email, with escaped purpose text |
| `crons/mod.rs` (M5) | The every-minute vault cron (`jobs/vault_cron.rs`): use deadlines, nonce expiry |

**Implements:** FR-VLT-8–13, FR-VLT-26–28, FR-VLT-30 and edge rows U22–U40, U64, U65, U71–U82, U84, U86.

**Acceptance:** the rows above; spike S-VLT3 against RFC 9421 Appendix B and the committed fixed vectors (the TypeScript, Python and Go helpers are checked against the same vectors in M46–M48, not here); every `conf::vault::*` fixture passes against the reference consumer
in CDP and proxy modes; `sdk::coverage::every_operation` passes with the new operations.

---

## M44 · One-time codes and sessions (after M43, M30 and M31; the SMS arm with M39)

M30 for the event-feed visibility of `vault.*` events; M31 because rule 4b sits beside the screening rules.

**Files:** `crates/core/src/vault/totp.rs`, `crates/worker/src/vault/{codes.rs, sessions.rs}`, `mailbox/ingest.rs`
(rule 4b), `mailbox/codes.rs` (`VaultTakeCode`), `handlers/wait.rs` (the strict-mode refusal), the SMS arm in
`mailbox/sms.rs` (`VaultTakeSmsCode`) and `phone/codes.rs` with M39. Mailbox columns `verifications.vault_item_id`,
`verifications.vault_use_id`, `verifications.vault_consumed_at`.

| File (owner milestone) | Hook |
|---|---|
| `mailbox/ingest.rs` (M7, M27) | Quarantine rule 4b after 4a; `IngestInput.approved_accounts[].vault_items` |
| `handlers/wait.rs` (M7) | `403 policy_denied` (`codes_into_fills_only`) |
| `jobs/retention.rs` (M14) | Hidden `vault_code` mail and code rows after 24 hours |

**Implements:** FR-VLT-14–17 and edge rows U41–U49, U51, U52, U70, U87.

**Acceptance:** the rows above; `live::vault::email_code_into_fill` on staging; `it::vault::sms_otp_phase` once M39
has landed.

---

## M45 · Rotation, signing, reveal, tooling and the Pylota cut-over (after M44)

**Files:** `crates/worker/src/vault/{versions.rs, sign.rs, reveal.rs}`, the console reveal and "Show a new value"
pages, `jobs/vault_cron.rs` (every minute: overlaps, re-wrap sweep; daily: session expiry, orphan objects, rotation
reminders), the session import route, CLI `revoke-all`, `idvy ops switch vault_redeem`, the runbook "Vault
incident" in [Observability § 9](design/observability.md#9-runbooks).

**Implements:** FR-VLT-18–20, FR-VLT-29 and edge rows U50, U53–U58, U88, U89.

**Acceptance:** the rows above; `live::vault::browserbase_settings`; a staging rehearsal of
[Vault § 22.3](design/vault.md#223-migrating-from-browser_sealed_sessions) with a test integrator (register and
activate consumers, import 20 sessions, load and write back, `revoke-all`); seven days of the vault telemetry on
staging with no audit append failure, no `vault_value_exposed_total` outside the fixtures and no unexplained
`placeholder_misplaced` before `PM_VAULT` is turned on in production.

---

## M46 · SDK conformance suite, phone and vault helpers (after M36, M38, M39 and M43)

**Goal.** One language-neutral proof of the client rules, generated from the Rust side, and the phone and vault
helpers in the Rust and TypeScript SDKs, so the Python and Go SDKs are built against data rather than prose.

**Files:** in this repository, `sdks/fixtures/conformance/**`, `crates/sdk/src/{phone.rs, vault.rs,
vault_consumer/{mod.rs, origin.rs, keys.rs, sign.rs, packet.rs, redact.rs}}`, `crates/sdk/tests/conformance.rs`,
`xtask/src/gen_sdk_vectors.rs`, `sdks/python-tests.json` and `sdks/go-tests.json` (empty inventories), the guide
`vault-consumers.md`. In `PILOTAAI/pylota-identity-typescript`: the conformance runner, the phone and vault
helpers and the `vault-consumer` entry point.

| File (owner milestone) | Hook |
|---|---|
| `crates/sdk/Cargo.toml`, `crates/api-types/Cargo.toml` (M16, M0) | `license = "Apache-2.0"` in place of the workspace licence, with `LICENSE` and `NOTICE` in each crate; `deny.toml` allows `Apache-2.0` for these two ([ADR 0022](adr/0022-sdk-languages.md) decision 9) |
| `crates/sdk/Cargo.toml` (M16) | Feature `vault-consumer` with `hpke` (the version the Worker pins), `zeroize` |
| `xtask/src/trace.rs` (M0) | Prefixes `py` and `go`, read from `sdks/python-tests.json` and `sdks/go-tests.json` as `ts::` names are; a missing name is `pending (SDK)` |
| `xtask` `release` (M19) | Assets `py-tests-required.json` and `go-tests-required.json`; `release sign --repo` accepts `PILOTAAI/pylota-identity-python` and `PILOTAAI/pylota-identity-go` with their trusted comments |
| `.github/workflows/ci.yml` (M19) | `cargo xtask gen-sdk-vectors --check` in the `test` job |

**Implements:** FR-SDK-28 – FR-SDK-31 and rows Y76–Y85, Y89 (the Rust and TypeScript parts).

**Acceptance:** `sdk::conformance::*`, `xtask::sdk_vectors_current`, the extended `xtask::release_sign_sdk_repo`;
in the TypeScript repository `ts::conformance::all`, `ts::package::y80_consumer_entry_point_separate`,
`ts::it::vault::y78_consumer_signature_accepted` and `ts::it::phone::y82_buy_retry_one_number`; the reference
consumer of [Vault § 24.3](design/vault.md#243-consumer-conformance-fixtures) built on the Rust helpers.

---

## M47 · Python SDK (after M46)

**Files:** the repository `PILOTAAI/pylota-identity-python` ([SDKs § 3.5](design/sdk-and-skills.md#35-python)); here, `sdks/python-tests.json`
(the inventory a release produces) and the guide `python-sdk.md`.

**Implements:** FR-SDK-25, FR-SDK-32 and FR-SDK-33 for Python, and rows Y72, Y75, Y80, Y86, Y87, Y89, Y91,
Y92 (Python parts).

**Human prerequisites.** Create `PILOTAAI/pylota-identity-python` (Apache-2.0); register the PyPI project
`pylota-identity` with a trusted publisher (repository `PILOTAAI/pylota-identity-python`, workflow
`release-publish.yml`, environment `release`), which reserves the name; disallow API tokens on the project;
give the repository a staging test key.

**Acceptance:** the `py::` rows above pass in the SDK repository's CI on CPython 3.11–3.15 and are listed in
`sdks/python-tests.json`; `py::conformance::all`; the first release publishes the sdist and wheel listed in its
owner-signed `SHA256SUMS`, with PyPI attestations; `py::doctest::site_examples`; M20 gains
`py::live::round_trip`.

---

## M48 · Go SDK (after M46)

**Files:** the repository `PILOTAAI/pylota-identity-go` ([SDKs § 3.6](design/sdk-and-skills.md#36-go)); here, `sdks/go-tests.json`, the guide
`go-sdk.md` and the Go section of the [SDKs](../reference/sdks.md) reference.

**Implements:** FR-SDK-26, FR-SDK-32 and FR-SDK-33 for Go, and rows Y73, Y74, Y80, Y86, Y88, Y90, Y91, Y92
(Go parts).

**Human prerequisites.** Create `PILOTAAI/pylota-identity-go` (Apache-2.0) with an environment `release`
whose token may push tags; give it a staging test key; keep the organisation name `PILOTAAI`.

**Acceptance:** the `go::` rows above pass on Go 1.26.10 and 1.27.3 and are listed in `sdks/go-tests.json`;
`go::conformance::all`; the first release's tag is pushed only after the owner's signature, and the proxy's
`h1:` equals the signed one; `go::doctest::site_examples`; M20 gains `go::live::round_trip`.

---

## M20 · Staging deploy and live proof

**Implements:** NFR-OPS-1 (the timed rehearsal, step 12) and the live measurements of NFR-REL-3,
NFR-PERF-4, NFR-OPS-2 and NFR-COST-1 (step 13). M17 Completion is accepted here too: its checks run in the
gate once every milestone it measures has landed.

Deploy to staging with `idvy setup` and `idvy deploy` from the docs alone, as if you were a new
self-hoster, using the latest release candidate that M19's pipeline published (`v1.0.0-rc.N`); each fix
found here ships as the next candidate. Then run `live::*`. Each step names its tests in [Testing §10](design/testing.md#10-live-end-to-end-suite-live);
the ones marked manual there need a person in a browser and run with `cargo xtask live --manual`:

1. Inbound from Gmail and Outlook test mailboxes. The verdicts are correct, and HTML-only mail
   produces text.
2. Outbound to both. Each reply threads correctly in the recipient's client, and replies come back
   into the same thread.
3. Bounce: a non-existent mailbox at a domain you control. Complaint: through the provider's simulator
   if available, otherwise a manual test.
4. A domain change: platform address, then a subdomain of the staging tenant zone, then that zone's apex
   (with the staging platform key), then rollback.
5. A domain on an external DNS host with `dns_records`: publish the records at a DNS provider other than
   Cloudflare, wait for `healthy`, receive from Gmail through SES, and send with aligned DKIM and SPF.
6. A domain failure: delete one of the three SES DKIM CNAMEs of step 5's domain through the external DNS
   provider's API (`dkim_missing`, fail). After two checks the domain is `failing`, sends fall back, the
   operator is told. Restore the record and the domain recovers. A Cloudflare zone's sending DKIM record
   cannot be used: Email Sending records stay locked for as long as the sending domain exists (Email Service
   [locked DNS records](https://developers.cloudflare.com/email-service/configuration/domains/), read
   2026-10-10).
7. Erasure of a counterparty with a held thread. The receipt is correct and the probes are empty.
8. An MCP client (Claude Code) connects, searches and sends with an idempotency key.
9. Console: sign in with a magic link, invite a second member, release a quarantined message, and see it
   in the audit log (`live::console::magic_link_invite_release`).
10. Cloud sign-up with Google (`PM_SIGNUP=open`): a new account and workspace, the Overview with its
    first-run checklist, and Checkout from `?plan=developer` (`live::signup::google_to_checkout`, manual).
11. Billing in Stripe test mode: upgrade Free to Developer through Checkout, spend the send allowance to a
    `402`, buy a top-up, and retry the same send successfully (`live::billing::upgrade_spend_topup_retry`,
    manual for the two Checkout pages). Staging runs with a `PM_PLAN_CATALOG` whose plans keep their
    names and use Stripe test-mode prices but have small allowances (Free 10 sends, Developer 20 sends, a
    sends top-up of 5), so the allowance is spent in a few sends; the production catalog is never used
    for this.
12. NFR-OPS-1, a fresh-account rehearsal (`live::ops::fresh_deploy_rehearsal`): a fresh agent session,
    started with no repository checkout, no memory and no context but `self-hosting.md` and a new
    Cloudflare account's credentials, deploys from that page alone; the hands-on time it reports (the
    commands and dashboard steps the page asks a person to do, excluding waiting on Cloudflare) is at
    most 15 minutes. The session is recorded (its transcript and timings go in the release notes), and
    every question it had to guess is fixed in the page before the release
    ([ADR 0015](adr/0015-solo-operator.md)). A paid external tester following the same page is an
    optional extra, not a requirement.
13. Measured on staging: the heartbeat workflow (`live::ops::heartbeat_workflow`); NFR-REL-3
    (`live::slo::inbound_to_webhook`), NFR-PERF-4 with the real models
    (the hybrid figure of `it::bench::hybrid_p95` repeated against staging), NFR-OPS-2
    (`live::ops::restore_drill`) and NFR-COST-1 (`live::ops::idle_cost_review` after a week of idling).
    NFR-REL-2 and NFR-REL-4 are read from the SLO dashboard over the live run.
14. Agent keys: an assertion minted on staging, with a tenant key that the staging platform key minted
    with `identities:sign` granted, verifies with `idvy assertions verify` against staging's
    JWKS, and stops verifying within 5 minutes of pausing the identity (`live::assertions::verify_then_pause`).
    With S13 passed and `PM_WEB_BOT_AUTH=on`, a signed request to
    `https://crawltest.com/cdn-cgi/web-bot-auth` returns `401` (the directory is not registered on
    staging; `live::http_signatures::crawltest_unregistered_401`).
15. Notifications: in Stripe test mode, with the staging catalog of step 11, sends cross 80% and an alert
    arrives once (`live::notify::usage_alert_once`); a `new_mail` notification reaches the Gmail test
    mailbox with no content from the mail (`live::notify::new_mail_no_content`); Gmail's one-click
    unsubscribe turns that kind off (`live::notify::gmail_one_click_unsubscribe`, manual).
16. The AgentMail-parity milestones, with the live tests their acceptance names: a scheduled draft send
    (`live::drafts::scheduled_send`, M29); the event stream across a deploy
    (`live::events::stream_through_deploy`, M30); a screening hold round trip
    (`live::screening::t1_hold_round_trip`, M31); real invitations, the client matrix and an applied reply
    (`live::calendar::inbound_real_invitations`, `live::calendar::client_matrix`,
    `live::calendar::reply_applied`, M32); a large attachment through an upload
    (`live::uploads::large_attachment_link`, M33); agent self-sign-up end to end and its one-click refusal
    headers (`live::signup::end_to_end`, `live::signup::x40_refuse_one_click_headers`, M34); a bounce that
    moves the rates (`live::metrics::bounce_moves_rates`, M35); a TypeScript SDK round trip
    (`ts::live::round_trip`, M36); Python and Go SDK round trips
    (`py::live::round_trip`, M47; `go::live::round_trip`, M48), each run by its SDK repository with a staging
    test key; and an ARC-sealed forward through Cloudflare and a `docs` link that
    resolves (`live::arc::cloudflare_forward_behaviour`, `live::docs::z78_anchor_resolves`, M37). Each is
    defined on its milestone's design page.
17. Phone numbers and the vault on staging, with the live tests their milestones name: a GB number bought,
    used for SMS both ways and released; a US number whose first send is refused before its 10DLC campaign
    is provisioned; a vault fill by a conformant browser consumer with a TOTP phase and a session
    write-back; each product's kill switches turned off and on again
    (`live::ops::phone_kill_switches`, `live::ops::vault_kill_switches`).

**v1.0 release criteria:** [PRD §9](prd.md#9-release-criteria-v10). `v1.0.0` is tagged on a commit that
changes only the workspace version of the last candidate that passed this milestone and adds
`release-gates/v1.0.0.md`, ticked and naming that candidate (criterion 7). Its `publish` job is the first to publish the crates to
crates.io, so an integrator that takes the SDK from crates.io waits for it.

---

## M28 · Cloud production commissioning (after M20)

**Files:** none in the repository. The owner keeps the rendered production `deploy/wrangler.toml`, the
check results and the gate notes in a private operations repository ([Cloud commissioning](cloud-commissioning.md)).

**Implements:** FR-OPS-5: the production configuration of [Cloud commissioning §4](cloud-commissioning.md#4-production-configuration)
and its default policy (§5), the scoped Cloudflare tokens (§3), Stripe live mode (§6), the OAuth
applications (§8), SES in `eu-west-2`, the Pylota partner and Pylota's two tenants (§7), and the three
phases of §2.

**Acceptance:**

- Gate A ([Cloud commissioning §10](cloud-commissioning.md#10-go-live-gates)): checks CC1–CC14, CC18 and CC19
  pass on production; the fresh-agent dry run finds no difference between the docs and the deployed
  configuration; the independent adversarial agent review finds nothing rated high; `PM_SIGNUP=waitlist`
  with no invitation sent. Pylota's backend creates operator workspaces with its partner key.
- Gate B: 14 days of Pylota on Cloud with no service-owned edge row failing in production; checks CC15–CC17;
  DMARC at `p=quarantine`. Waitlist invitations start.
- Gate C: the [pre-release checklist](design/security.md#16-pre-release-checklist) complete; 30 days of
  phase B within the auto-pause thresholds; DMARC at `p=reject`; a second dry run and review on the
  phase-C configuration. `PM_SIGNUP=open`: Pylota Identity Cloud is public.
- Gate P ([Cloud commissioning §10](cloud-commissioning.md#10-go-live-gates)), stage by stage: checks
  CC20–CC27; the Telnyx steps of [§11](cloud-commissioning.md#11-phone-numbers-the-telnyx-account) for the
  countries offered; the phone rows of [Launch readiness §3.3](launch-readiness.md#33-phone-numbers) `done`.
  `PM_PHONE=on`; `phone.allowed` on for Pylota's first operator, then every Pylota operator, then others.
- Gate V: checks CC28–CC32; Pylota's consumers activated after their conformance fixtures; the vault rows of
  [Launch readiness §3.4](launch-readiness.md#34-vault) `done`. `PM_VAULT=on`; `vault.allowed` on for
  Pylota's tenants, then others.
- Gate S: check CC33 on the first full Python and Go SDK releases.

---

## M49 · Launch (after M28, M41, M45 and M48)

**Files:** `docs/src/project/launch-readiness.md` (exists; statuses updated as rows close),
`xtask/src/launch.rs` (parses the tracker's tables), `release-gates/v1.0.0.md`, and in the private operations
repository the gate notes, signed legal documents and review reports the tracker names.

| File (owner milestone) | Hook |
|---|---|
| `xtask/src/main.rs`, the `release-gate` command (M19) | Reads `docs/src/project/launch-readiness.md` and refuses `v1.0.0` while a row whose **Blocks** names `v1.0` is neither `done` nor `n/a` |
| `crates/cli/src/commands/doctor.rs` (M16) | A `launch` section: for each flag that is on (`PM_SIGNUP` phase, `PM_PHONE`, `PM_VAULT`), warns when a row that blocks its gate is not `done`, reading the tracker published with the release |

**Implements:** FR-OPS-6 and NFR-OPS-4.

**Acceptance:**

- `xtask::launch_readiness_refuses_open_blockers`: a tracker with a `v1.0` row `open` makes `release-gate`
  fail with the row's ID; the same tracker with it `done` and an evidence link passes; a row without an
  owner, evidence or status is a parse error.
- `cli::doctor::launch_readiness_gates`: with `PM_PHONE=on` and a `P1` row `open`, `idvy doctor` prints a
  `warn` naming the row.
- Every row of [Launch readiness](launch-readiness.md) is `done` or `n/a` for the gates opened, each with
  evidence a later reader can check: the legal pages and sub-processor list live (CC20), counsel's answers
  recorded for the questions that block an opened gate, the tax decisions recorded, the licence rows done,
  the security sign-off rows done, the operations rows done, and each opened gate's first-week note written
  ([Launch readiness §11](launch-readiness.md#11-launch-day-and-the-first-week)).
- A fresh agent that never saw the deployment reads [Launch readiness](launch-readiness.md) and
  [Cloud commissioning](cloud-commissioning.md) alone and reports every row it cannot verify from the
  evidence named; the owner fixes each before the milestone is accepted.
