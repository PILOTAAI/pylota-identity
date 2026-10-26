Where this goes: `docs/src/project/build-plan.md`. Four insertions, each introduced by where it goes. Milestone numbers are this design's own (M42–M45); the coordinator renumbers them at integration and keeps the cross-references (the phone design's M41 depends on M42 for managed secrets; its M39 shares the SMS code arm with M44).

## 1. In "## Timeline", phase B row: after the phone sentence ("M38–M41 (phone numbers, …) …"), add

; M42–M45 (the vault, [ADR 0021](adr/0021-agent-vault.md)) follow M5, M6, M14, M15, M16, M17 Foundation, M21, M25, M27, M30 and M31

## 2. In "## Dependency graph", in the second `text` block, add before the line "M20 staging + live proof ─▶ M28 Cloud production commissioning (phases A, B, C)"

```text
M5, M6, M14, M17 Foundation, M27 ─▶ M42 vault core: keys, items, audit, erasure ─▶ M20
   (spikes S-VLT1 and S-VLT2 first; M42 adds the vault steps to M14's jobs and six permissions to M5's router)
M42, M15, M16, M21, M25 ─▶ M43 grants, uses, approval, consumers, tickets ─▶ M20
   (two Decision arms in M5's auth/people.rs; six tools to M15; the conformance fixtures in crates/conformance)
M43, M30, M31 ─▶ M44 one-time codes and sessions ─▶ M20   (its SMS arm lands with whichever of M44 and M39 comes second)
M44 ─▶ M45 rotation, signing, reveal, tooling and the Pylota cut-over ─▶ M20
   (production keeps PM_VAULT off until M45 is accepted)
```

## 3. In "**Shared files.**", add to these bullets

- In the `jobs/erasure.rs` bullet, after the phone milestones: ", M42 (the vault steps of identity and tenant
  erasure, custody slots first, and the receipt fields `vault_items_deleted`, `vault_versions_shredded`,
  `vault_keys_shredded`, `vault_uses_deleted`, `vault_audit_deleted`)".
- In the `jobs/retention.rs` bullet, after the phone milestones: ", M42 (the `vault` step of the global
  retention job: terminal grants and uses after 90 days, audit by age after 400 days with anchors, revoked
  consumers after 400 days)".
- In the `crates/core/src/sealed.rs` bullet, after the phone milestones: ", M42 (`vault_keys.private_sealed` and
  `vault_custody_slots.private_wrapped`, whose inner layer the sweep carries unopened)".
- In the `mailbox/ingest.rs` bullet (or a new one): "M44 adds quarantine rule 4b and the `vault_code` reason;
  `mailbox/codes.rs` gains `VaultTakeCode`."
- New bullet: "`auth/people.rs`: M43 adds `Decision::ApproveVault` and `Decision::ActivateConsumer`."
- New bullet: "`auth/routespec.rs`: M42 adds `Auth::ConsumerSignature` for the five consumer routes (wired in
  M43) and the six `vault:*` permissions with the identity-level refusals of
  [Vault § 17.2](design/vault.md#172-what-each-kind-of-key-may-hold)."

## 4. New milestones, after the phone milestones (M41) and before "## M20 · Staging deploy and live proof"

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
U85.

**Acceptance:** the rows above with their tests; `xtask::vault_types_have_no_debug` and
`xtask::docs_forbidden_claims` run in CI; `it::security::cross_tenant_matrix` covers the new routes.

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

**Implements:** FR-VLT-8–13, FR-VLT-26–28, FR-VLT-30 and edge rows U22–U40, U64, U65, U71–U82, U84.

**Acceptance:** the rows above; spike S-VLT3; every `conf::vault::*` fixture passes against the reference consumer
in CDP and proxy modes; `sdk::coverage::every_operation` passes with the new operations.

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

**Implements:** FR-VLT-14–17 and edge rows U41–U49, U51, U52, U70.

**Acceptance:** the rows above; `live::vault::email_code_into_fill` on staging; `it::vault::sms_otp_phase` once M39
has landed.

## M45 · Rotation, signing, reveal, tooling and the Pylota cut-over (after M44)

**Files:** `crates/worker/src/vault/{versions.rs, sign.rs, reveal.rs}`, the console reveal and "Show a new value"
pages, `jobs/vault_cron.rs` (every minute: overlaps, re-wrap sweep; daily: session expiry, orphan objects, rotation
reminders), the session import route, CLI `revoke-all`, `idvy ops switch vault_redeem`, the runbook "Vault
incident" in [Observability § 9](design/observability.md#9-runbooks).

**Implements:** FR-VLT-18–20, FR-VLT-29 and edge rows U50, U53–U58.

**Acceptance:** the rows above; `live::vault::browserbase_settings`; a staging rehearsal of
[Vault § 22.3](design/vault.md#223-migrating-from-browser_sealed_sessions) with a test integrator (register and
activate consumers, import 20 sessions, load and write back, `revoke-all`); seven days of the vault telemetry on
staging with no audit append failure, no `vault_value_exposed_total` outside the fixtures and no unexplained
`placeholder_misplaced` before `PM_VAULT` is turned on in production.
