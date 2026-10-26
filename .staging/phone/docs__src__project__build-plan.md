Where this goes: `docs/src/project/build-plan.md`. Four insertions, each introduced by where it goes. Milestone numbers are this design's own (M38–M41); the coordinator renumbers them at integration and should keep the cross-references (the vault's core milestone is M42 in the vault design).

## 1. In "## Timeline", phase B row: after "M29–M37 (the AgentMail-parity features, …) follow the core tracks they extend", add

; M38–M41 (phone numbers, [ADR 0020](adr/0020-phone-numbers.md)) follow M5, M6, M8, M14, M17 Foundation, M21, M22, M27, M30 and M31, and M41 also the vault's core milestone

## 2. In "## Dependency graph", in the second `text` block, add before the line "M20 staging + live proof ─▶ M28 Cloud production commissioning (phases A, B, C)"

```text
M5, M6, M8, M14, M17 Foundation, M21, M22, M27 ─▶ M38 phone numbers, requests and compliance ─▶ M20
   (M38 adds Q_PHONE to M5's template and three QuotaRequest variants, both named exceptions; four tools to
    M15 and commands to M16; the standing budget and one-call purchase land here)
M38, M30, M31 ─▶ M39 SMS ─▶ M20   (its vault code arm lands with whichever of M39 and the vault core comes second)
M38 ─▶ M40 voice routes and calls ─▶ M20
M38, M39, M40, vault core ─▶ M41 porting, port-out guard, reconciliation, invoicing and migration ─▶ M20
   (production keeps PM_PHONE off until M41 is accepted)
```

## 3. In "**Shared files.**", add to these bullets

- In the `quota/mod.rs` bullet, after "The one exception is M35, …": "M38 is a second exception: it adds
  `PhoneSpendHold`, `PhoneSpendSettle` and `PhoneSpendRelease` and the counters `phone_spend:*`,
  `phone_spend:reg:*`, `phone_budget:t:*` and `phone_search` ([Phone numbers § 18.3](design/phone-numbers.md#183-spend-caps),
  [§ 5.7](design/phone-numbers.md#57-the-standing-purchase-budget))."
- In the `jobs/erasure.rs` bullet, after "M37 (…)": "and M38 (the `phone_release` step, the compliance-document
  steps and the phone receipt fields), M39 (the SMS rows) and M40 (the call rows)".
- In the `jobs/retention.rs` bullet, after "M30 (the event-log steps)": ", M38 (phone requests, provider events,
  the message index, charges, KYC retention and onboarding drafts) and M39 (SMS verification rows)".
- In the `deploy/wrangler.toml.tmpl` bullet, after the M31 exception: "M38 is the second exception: it adds
  `Q_PHONE`, the `pm-phone` consumer, `pm-phone-dlq` and the R2 lifecycle rule for `phone-staging/`."
- In the `crates/core/src/sealed.rs` bullet, after "M37 (…)": ", M38 (`phone_tenants.sip_sealed`,
  `phone_compliance_profiles.tax_ids_sealed` and `representative_sealed`, `phone_compliance_documents.key_sealed`,
  `phone_requests.secrets_sealed`) and M40 (`phone_voice_connectors.api_key_sealed`,
  `phone_voice_routes.server_secret_sealed`)".
- New bullet: "`auth/people.rs`: M38 adds `Decision::ApprovePhoneRequest`, `Decision::DecidePortOut` and
  `Decision::SetPhoneBudget`."

## 4. New milestones, after "## M37 · Parity additions …" and before "## M20 · Staging deploy and live proof"

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
P73–P76, P98, P99, P101, P103, P104, P108–P114, P117, P118, P120–P123, P127.

**Order.** The first pull request is `live::phone::order_contract` and `live::phone::webhook_signature` with the
pure mapping tables, so the status tables rest on the real API before any state machine is built.

**Acceptance:** the rows above with their tests; `it::phone::scope_matrix` joins
`it::security::cross_tenant_matrix`; `sdk::coverage::every_operation` passes with the new operations.

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
| `vault/*` (vault milestone M42) | `SmsCodeSource` and rule 2b land with whichever of M39 and M42 comes second, with `it::phone::vault_code_withheld` and `it::vault::sms_otp_phase` |
| `mcp/tools.rs` (M15) | `phone_send_sms`, `phone_list_sms`, `phone_get_sms`, `phone_wait_sms`, `phone_record_opt_out` |

**Implements:** FR-PHN-21 to FR-PHN-31, FR-PHN-45 (`outbound_sms` with registrations), FR-PHN-48, FR-PHN-49 (US
onboarding, drafts for 10DLC), FR-PHN-52 (texting from US numbers and cross-border destinations), FR-PHN-54 (the
sole-proprietor brand, where the decision allows it), and edge rows P42–P72, P77 (SMS charges in the job's input),
P80–P84, P105, P106, P115, P116, P119, P124 (SMS half), P125.

**Acceptance:** the rows above; `live::phone::send_idempotency` decides whether the repeats of [Phone numbers § 12.3](design/phone-numbers.md#123-dispatch-provider-idempotency-and-uncertain) are enabled;
`live::phone::redaction_levels` recorded in [Phone numbers § 8](design/phone-numbers.md#8-tenant-provider-resources); `live::phone::a2p_paths`, `live::phone::sole_prop_otp`,
`live::phone::cross_border_sms` and `live::phone::us_onboarding_end_to_end` pass on staging (the last gates step 5
of [§2.5](design/phone-numbers.md#25-us-enablement), and its measured waits replace the catalogue's `typical_wait_days`);
`live::phone::uk_brand_registration` is run and its result recorded, whatever it is.

## M40 · Voice routes and calls (after M38)

**Files:** `crates/core/src/phone/voice.rs`, `crates/worker/src/phone/{voice.rs, vapi.rs}`,
`handlers/phone/{routes.rs, connectors.rs, calls.rs}`, `mailbox/calls.rs`, CLI `connectors`, `route`, `calls`,
`usage`; MCP `phone_place_call`, `phone_list_calls`.

**Implements:** FR-PHN-32 to FR-PHN-35, FR-PHN-52 (calls from US numbers for tenants registered elsewhere) and
edge rows P85–P91, P124 (calls half).

**Acceptance:** the rows above; `live::phone::vapi_route`, `live::phone::vapi_call_list`,
`live::phone::forwarding` and `live::phone::e911_address` pass on staging.

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
