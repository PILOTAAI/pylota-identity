# Phone numbers

Binding for implementation. An identity can hold **phone numbers** as well as email addresses: a number
is another address of the identity, bought in the tenant's own legal name, that receives and sends SMS
and carries voice calls to a route the tenant chooses. Pylota Identity is not a carrier. It buys numbers
from one provider, Telnyx, behind a provider trait, and adds what an agentic OS needs on top: a person
approves every purchase (one by one, or once through a standing budget the person sets), and every port,
release, KYC submission and registration fee; once a tenant has finished a one-time set-up per country, an
agent gets a number with one call ([§6.7](#67-one-call-purchase)); every billable provider call
has its own idempotency, a spend-intent record and a reconciler, so a retry never buys two numbers or sends
two texts; KYC documents are encrypted, write-only and erased with receipts; inbound SMS is untrusted input,
screened like mail; verification codes can go to the vault without passing through the model; and a
port-out cannot take a number away without a person deciding. The page also gives Pylota everything its
own Telnyx integration does today (its parity list, [§31](#31-pylota-agentic-os-parity-and-migration)),
and builds in the lessons of a review of that integration. The feature is off unless the operator sets
`PM_PHONE = "on"` ([§30](#30-rollout-telemetry-and-rollback)); it is a T2 surface (money, KYC and personal
data, tenant isolation, provider credentials) ([ADR 0020](../adr/0020-phone-numbers.md)).

| | |
|---|---|
| Requirements | FR-PHN-1 to FR-PHN-54 (this page, [§33](#33-functional-requirements)); it extends FR-IDN-2, FR-IDN-4, FR-IDN-10, FR-IN-5, FR-IN-11, FR-SCR-1, FR-PRV-1 to FR-PRV-6, FR-BILL-1 to FR-BILL-6, FR-TEN-4, FR-CON-6 and FR-KEY-4 |
| Edge cases | [P1](#34-edge-cases)–[P131](#34-edge-cases) (section `P · Phone numbers` of the [edge-case register](../edge-cases.md); P107 is integrator-owned) |
| Code | `crates/core/src/phone/{mod.rs, e164.rs, catalogue.rs, requests.rs, budget.rs, onboarding.rs, capabilities.rs, numbers.rs, orders.rs, sms.rs, segments.rs, consent.rs, verify.rs, a2p.rs, voice.rs, porting.rs, money.rs, telnyx.rs}` (validation, state machines, the quote digest, the standing budget, onboarding status, capability facets, the policy pipeline, segment counting, status and error mapping from Telnyx; pure), `crates/core/data/phone-catalogue.json`, `crates/core/data/phone-rate-card.json`; `crates/worker/src/phone/{mod.rs, provider.rs, telnyx/{client.rs, webhook.rs, mapping.rs}, vapi.rs, fake.rs, execute.rs, buy.rs, onboarding.rs, orders.rs, numbers.rs, tenant_resources.rs, compliance.rs, documents.rs, sms_out.rs, sms_in.rs, a2p.rs, voice.rs, porting.rs, port_out.rs, reconcile.rs, money.rs, invoicing.rs}`; `handlers/phone/{catalogue.rs, numbers.rs, buy.rs, setup.rs, budget.rs, requests.rs, compliance.rs, sms.rs, wait.rs, calls.rs, consent.rs, routes.rs, connectors.rs, port_outs.rs, charges.rs, platform.rs}`; `handlers/hooks_telnyx.rs`; `consumers/phone.rs` (the `pm-phone` consumer); `mailbox/sms.rs`, `mailbox/calls.rs`; `crons/phone.rs`; `jobs/phone_reconcile.rs`; `console/pages/phone/*.rs`; `mcp/tools.rs` (twelve tools); `crates/cli/src/commands/phone.rs`; hooks in other milestones' files listed in [§36](#36-milestones) |
| Tables | D1: `phone_tenants`, `phone_numbers`, `phone_number_releases`, `phone_requests`, `phone_onboarding_drafts`, `phone_compliance_profiles`, `phone_compliance_documents`, `phone_requirement_groups`, `phone_registrations`, `phone_voice_connectors`, `phone_voice_routes`, `phone_port_outs`, `phone_consent`, `phone_charges`, `phone_provider_events`, `phone_message_index` (new); `service_accounts.sms_senders_json` (new column). Mailbox: `sms_conversations`, `sms_messages`, `sms_media`, `sms_verifications` (with `vault_item_id`, `vault_consumed_at`, `vault_use_id` for the vault's code source), `calls` (new); `idempotency.sms_id` (new column). `TenantQuota`: counters `phone_spend:*`, `sms:*`, holds of feature `phone_spend`. R2: `phone-staging/…`, `t/{ten}/phone/docs/{cpd}`, `t/{ten}/i/{idn}/sms/{sms}/m/{n}` ([§24](#24-privacy), [Data model](data-model.md)) |
| Contracts | The routes of [§28](#28-mcp-and-cli) and the [REST API](../../reference/api.md) (catalogue, setup, budget, one-call purchase, search, requests, numbers, routes, connectors, compliance, documents, SMS, `sms/wait`, calls, consent, port-outs, charges, platform reconcile and adopt); `POST /hooks/telnyx` (no key; Ed25519 signature); the permissions `phone:read`, `phone:request`, `phone:send`, `phone:call`, `phone:approve`, `phone:manage`, `phone:compliance`; the events of [§25](#25-events-and-audit); the policy fields of [§19](#19-policy), among them the standing budget `phone.auto_approve` and the registration cap; the errors of [§22](#22-failure-handling); the ID prefixes `pnm_ phr_ pdr_ cpp_ cpd_ sms_ smc_ cal_ pvc_ vrt_ pto_ pti_ a2p_ chg_ pev_` (`pvc_` for voice connectors, because the vault design uses `vcn_` for its consumers); the variables `PM_PHONE`, `PM_TELNYX_API_KEY`, `PM_TELNYX_PUBLIC_KEY`, `PM_TELNYX_API_BASE`, `PM_TELNYX_ACCOUNT_ID`, `PM_PHONE_CATALOGUE`, `PM_PHONE_RATE_CARD`, `PM_PHONE_KYC_RETENTION_DAYS`; the queue binding `Q_PHONE` (queues `pm-phone`, `pm-phone-dlq`); the ops switch `phone` |
| Limits | [§26](#26-limits), to be copied to [Limits › Phone numbers](../../reference/limits.md) |
| Built in | [M38](#m38--phone-numbers-requests-and-compliance-after-m5-m6-m8-m14-m17-foundation-m21-m22-and-m27) (numbers, requests, compliance, money core), [M39](#m39--sms-after-m38-m30-and-m31) (SMS, consent, verification, US A2P), [M40](#m40--voice-routes-and-calls-after-m38) (voice routes, calls, emergency settings), [M41](#m41--porting-port-out-guard-reconciliation-invoicing-and-migration-after-m38-m39-m40-and-the-vault-core-m42) (porting, the port-out guard, the nightly reconciler, invoicing, Pylota's migration) |

**Sources.** Telnyx facts come from the Telnyx OpenAPI document `team-telnyx/openapi` `openapi/spec3.json` at
commit `72cf10d738ff` (SHA-256 `fb64cb08…6428eb`), the developer documentation at
`developers.telnyx.com` (the `llms.txt` index and its full-text section files), the support articles and
legal pages named where used, and the public pricing API `api.telnyx.com/v2/pricing/products/{slug}`, all
read on 2026-10-24. Where Telnyx's own pages disagree, the page says **Conflict** and designs for the
stricter reading. Vapi facts come from `docs.vapi.ai` (Telnyx SIP integration guide and the
`POST /phone-number` API reference), read on 2026-10-24. Stripe facts come from `docs.stripe.com` (invoice
items), read on 2026-10-24. Regulatory statements name their source (Ofcom, the ICO, the FCC, the EU AI Act)
and the date read; every legal conclusion is left to counsel ([§32](#32-questions-for-counsel)). The
exchange rate is the ECB reference rate of 2026-10-23: 1 EUR = 1.1193 USD = 0.83728 GBP, so
**1 USD = £0.748**. Pylota's current integration was reviewed privately on 2026-10-24. Every Telnyx fact used in code is re-checked against the live API by the contract tests of
[§35](#35-tests) before the code that relies on it merges.

## 1. What a phone number is here

**An address of an identity.** A number belongs to exactly one identity, as an email address does. An
identity may hold several numbers (a UK mobile for texts, a geographic number for a branch's calls, a US
number later). When a number is released, ported out or moved, the identity keeps its SMS and call history,
and the number's row stays as a record of the ownership period ([§7](#7-numbers-on-identities)). Threads of
SMS are per number and counterparty; they are not mixed into mail threads.

**In the tenant's own legal name.** Every number is registered with the provider under the tenant's
compliance profile: its company name, registration and addresses, never TREFT LTD's
([§4](#4-compliance-profiles-and-kyc-documents)). TREFT is the reseller of record with Telnyx and keeps the
KYC records the reseller terms require; it never sub-allocates numbers registered in its own name.

**Business end users only; sole traders refused (owner decision, 2026-10-26).** A tenant's compliance
profile describes a business (a company, a limited liability partnership or a registered partnership). Several number types are
business-only (Ireland mobile, German and French toll-free), and individual KYC multiplies the personal data
held (Telnyx DID requirement articles for IE, DE, FR, read 2026-10-24). The owner chose to refuse **sole
traders** ([§1.1](#11-sole-traders-refused-owner-decision-2026-10-26), option (c)): a sole-trader profile is refused
(`422 compliance_incomplete`, `details.reason = "entity_type"`), as an individual's always is.

**Agents ask; people decide; set-up is once.** An agent can search for numbers, buy one inside a standing
budget a person set ([§5.7](#57-the-standing-purchase-budget)) or ask for one outside it, send SMS within
policy, place calls within policy or approval, and wait for a code. It can never port, release (other than a
release its purchase already scheduled, [§6.8](#68-throwaway-numbers)), submit KYC, pay a registration fee,
change where calls go or set its own budget. Those are **phone requests** that a person approves
([§5](#5-phone-requests-approval-for-everything-that-costs-money-or-cannot-be-undone)). The slow, legally
required steps (the tenant's regulatory identity for a country, the US texting registration) happen once per
tenant and country in **phone onboarding** ([§4.7](#47-phone-onboarding)), so a later purchase is one call
([§6.7](#67-one-call-purchase)). No agent and no
tenant ever holds a Telnyx credential; the Telnyx key is a Worker secret, and the provider's hosted MCP
server, whose one generic `invoke_api_endpoint` tool reaches every billable operation, is never exposed
(Telnyx remote MCP documentation, read 2026-10-24).

**Not in v1.** Call Control and media streaming routes (designed as a later step in
[§14.1](#141-routes-what-v1-offers-and-what-waits)); alphanumeric sender IDs (one-way, and in Ireland an
ISV cannot register them for its clients: Telnyx Ireland SMS guideline, read 2026-10-24); outbound MMS
(Telnyx documents MMS only from US long codes and short codes; inbound MMS is kept); number blocks and
bulk orders; WhatsApp, RCS and iMessage; Telnyx AI Assistants; a phone-location feature; individual end
users; a second provider (the trait is designed for one, [§2.2](#22-the-phoneprovider-trait)); managed
accounts per partner (designed for, [§2.1](#21-one-account-and-provider_account_id-everywhere)).

**Never.** AgentPhone, whose terms forbid reselling raw access without a written reseller agreement
(agentphone.ai/terms, read 2026-10-24), and Vonage, whose terms forbid onward allocation "except for use
within their own customer application" (vonage.com/legal/communications-apis/terms-of-use, read through
search on 2026-10-24). Holding number ranges from a regulator (Ofcom, ComReg, BNetzA, ARCEP, CNMC, AGCOM,
ACM) is out of scope: the licensing, emergency and interception duties do not fit a service run by one
person ([ADR 0015](../adr/0015-solo-operator.md)).

### 1.1 Sole traders: refused (owner decision, 2026-10-26)

Independent businesses are often sole traders, and the first tenants to hold numbers (Pylota's car-rental
operators, [§31](#31-pylota-agentic-os-parity-and-migration)) include them. **The owner chose option (c) on
2026-10-26** ([ADR 0020](../adr/0020-phone-numbers.md) decision 15): sole traders are refused for every row,
GB and US, and adopted numbers of sole-trader tenants take the dated exit below. Options (a) and (b) are kept
here so a later decision can revisit them; counsel's question 10 ([§32](#32-questions-for-counsel)) now asks
only whether the refusal is consistent with the rules that bind the tenants. The options that were weighed:

| Option | Effect | Cost |
|---|---|---|
| (a) Allow `sole_trader` profiles for GB and US rows | A sole trader's profile holds the proprietor's name, trading name, address and, for the US, the details of a sole-proprietor 10DLC brand ([§13.1](#131-us-sole-proprietor-brands)); the proprietor is the representative, whose data is sealed as today | More personal data held and erased; per-country rules for individuals to map; counsel's answer on question 10 |
| (b) Allow `sole_trader` for the US only | US sole-proprietor brands only (one number, an SMS one-time code to the proprietor's US or Canadian mobile); GB stays business-only | GB sole traders get no number from Pylota Identity |
| (c) Refuse sole traders (**chosen**, 2026-10-26) | New purchases need a business profile | Sole-trader tenants cannot buy, and adopted numbers need the exit path below |

**Adoption never strands a number.** Under every option, a number adopted from a platform-wide group
([§7.6](#76-adoption-for-pylotas-migration)) whose tenant cannot have an eligible profile keeps working, with
a dated exit:

1. The number's `legacy_deadline` is the later of its adoption plus 90 days and the decision's date plus 90
   days, so never before **2027-01-24** (2026-10-26 + 90 days); the 60-, 30- and 7-day reminders count down to
   it, and everything else about the number works until then.
2. Before the deadline the tenant either re-papers under an eligible profile, or **ports the number out** to a
   carrier of its own choosing (its own mobile contract keeps the number). Pylota Identity helps with the
   port-out: a person sees the PIN in the console ([§16](#16-the-port-out-guard)) and the port-out is
   authorised the moment it arrives, because the tenant asked for it.
3. At the deadline a number still legacy is **not released**. It moves to `suspended` with
   `reason = "legacy_unpapered"`: inbound keeps working (the number's owner can still be reached and can still
   port it away), outbound SMS and calls are refused with `409 phone_number_not_ready`
   (`details.reason = "legacy_unpapered"`), and the monthly charge continues until the tenant re-papers, ports
   out or approves a release. No timer ever releases a number.

## 2. The provider

### 2.1 One account, and `provider_account_id` everywhere

Pylota Identity Cloud starts with **one Telnyx account** owned by TREFT LTD. Tenants are separated inside it
by their own messaging profile, SIP connection, outbound voice profile, billing group, requirement groups
and tags ([§8](#8-tenant-provider-resources)). This is option A of the Telnyx study: it needs no commitment
plan, one key and one webhook key, and it is the only option on a pay-as-you-go account, because managed
accounts are "not available on pay-as-you-go" and start at a commitment of $1,000 to $2,000 a month (Telnyx
Managed Accounts support article 4951492, read 2026-10-24; **Conflict**: `pricing.md` lists a $500 committed
plan). Its costs are shared limits (dial rate, account suspension, one data-locality choice), which
[§30](#30-rollout-telemetry-and-rollback) and [§23](#23-security-t2) address.

Every row that names a provider resource stores `provider` (`telnyx`) and `provider_account_id` (the Telnyx
organisation ID, `PM_TELNYX_ACCOUNT_ID`, checked against `GET /v2/balance`'s account at isolate start). A move
to one managed account per partner later is a change of configuration and a per-row value, not a migration:
the client sends `x-managed-account-id: <provider_account_id>` when the row's account is not the manager's
own (Telnyx release note "Control managed accounts via API key", 2023-06-19, read 2026-10-24; the header is
not declared in the OpenAPI document, so a contract test covers it before it is used). Whether numbers can
move between accounts without porting is not documented (open question for Telnyx,
[§39](#39-open-questions-for-the-owner)).

**Prerequisites on the account**, checked by `idvy setup phone` and `idvy doctor`
([§2.4](#24-deployment-set-up)):

- the **Verified** account level. The Paid level orders only local numbers in the account's own country, has
  no 10DLC, no toll-free verification and no hosted messaging, caps outbound calls at 100 a day, and plays
  "This is an automated call generated on the Telnyx platform…" before machine-generated speech (Telnyx
  developer docs, account levels, read 2026-10-24). Verified needs KYC of TREFT LTD and an "AI agent
  evaluation" whose content is not published;
- the Telnyx reseller requirements annex accepted by TREFT, a contractual act for the owner, not code
  ([§32](#32-questions-for-counsel));
- data locality set once, by the owner, in the Telnyx portal. EU (Germany) storage covers CDRs, MDRs,
  recordings and speech-to-text; the setting can be changed once and never reversed; whether KYC documents
  and addresses follow it is not documented (Telnyx data locality documentation, read 2026-10-24). The
  recommendation is EU, because the first tenants are in the UK and Ireland ([§39](#39-open-questions-for-the-owner)).

**Status on Pylota Identity Cloud** (reported by the owner on 2026-10-26): TREFT's Telnyx account orders and
runs GB mobile numbers today; US is pending. The account level is not assumed from that report:
`idvy setup phone` probes it (a Verified-only read answered with 10038 means not Verified) and records the
result in `platform_state.phone_account` as `{level, probed_at, us_enabled}` through `POST /v1/platform/phone/probe`. A catalogue row whose country
needs the Verified level reports `eligible: false` with `reason = "platform_prerequisite"` in
`GET /v1/phone/countries` and in the setup status ([§4.7](#47-phone-onboarding)) until the probe passes and
the operator turns the row on ([§2.5](#25-us-enablement)).

### 2.2 The `PhoneProvider` trait

The provider is a trait in `crates/worker/src/phone/provider.rs`, as the mail transport is
([Outbound › The MailTransport trait](outbound.md)): the HTTP goes through `platform::Http`, every request
and response is mapped by pure functions in `crates/core/src/phone/telnyx.rs`, and `phone/fake.rs` is an
in-memory provider with fault injection for tests. A second provider (Twilio has the closest compliance
model: bundles of end user, documents and addresses per country, number type and end-user type, and
subaccounts; Twilio regulatory bundles documentation, read through search on 2026-10-24) is a second
implementation, not a redesign.

```rust
// crates/worker/src/phone/provider.rs
pub trait PhoneProvider {
    fn id(&self) -> ProviderId;                                   // Telnyx in v1
    // inventory and orders
    async fn search(&self, a: &Account, q: &NumberQuery) -> Result<Vec<Candidate>, ProviderError>;
    async fn order(&self, a: &Account, o: &OrderRequest) -> Result<OrderAck, ProviderError>;
    async fn orders_by_reference(&self, a: &Account, customer_reference: &str) -> Result<Listing<OrderState>, ProviderError>;
    async fn order_state(&self, a: &Account, order_id: &str) -> Result<OrderState, ProviderError>;
    async fn numbers(&self, a: &Account, page: &PageRequest) -> Result<Listing<NumberState>, ProviderError>;
    async fn number(&self, a: &Account, number_id: &str) -> Result<Option<NumberState>, ProviderError>;
    async fn configure_number(&self, a: &Account, number_id: &str, c: &NumberConfig) -> Result<NumberState, ProviderError>;
    async fn release_number(&self, a: &Account, number_id: &str) -> Result<ReleaseOutcome, ProviderError>;
    // tenant resources, compliance, porting, A2P, messaging
    async fn ensure_tenant_resources(&self, a: &Account, t: &TenantSetup) -> Result<TenantResources, ProviderError>;
    async fn requirements(&self, a: &Account, k: &RequirementKey) -> Result<RequirementSet, ProviderError>;
    async fn submit_compliance(&self, a: &Account, s: &ComplianceSubmission) -> Result<GroupState, ProviderError>;
    async fn group_state(&self, a: &Account, group_id: &str) -> Result<GroupState, ProviderError>;
    async fn port_in(&self, a: &Account, p: &PortInRequest) -> Result<Vec<PortOrderState>, ProviderError>;
    async fn decide_port_out(&self, a: &Account, id: &str, d: &PortOutDecision) -> Result<PortOutState, ProviderError>;
    async fn a2p(&self, a: &Account, op: &A2pOperation) -> Result<A2pState, ProviderError>;
    async fn send_sms(&self, a: &Account, m: &SmsSend, idempotency_key: &str) -> Result<SmsAck, ProviderError>;
    // events
    fn verify_webhook(&self, headers: &Headers, raw: &[u8], now_ms: i64) -> Result<ProviderEvent, WebhookError>;
}

pub struct Listing<T> { pub items: Vec<T>, pub complete: bool }  // complete = every page read and the
                                                                // provider's total equals items.len()
pub enum ProviderError {
    Definitive { http: u16, code: String },   // the provider refused; nothing was created
    Transient { retry_after_ms: Option<u64> },// refused before any change (429, a 503 the provider says is safe)
    Unknown { cause: UnknownCause },          // timeout, connection lost, 5xx on a POST that creates or charges
    Config { http: u16 },                     // 401, 403, 404 on a configured resource: not_configured
    Billing { code: String },                 // 402 or code 20100: carrier_billing
    NotPermitted { code: String },            // 10038 or 10039: the account level does not allow it
}
```

Rules of the trait:

- **Absence is concluded only from a complete listing.** A `Listing` with `complete: false` (a page cap,
  `meta.total_results` above the items read, a page that failed) never means "not found" (a Pylota parity
  rule).
- **`Unknown` is never retried as a new request** on an operation that creates or charges. It makes the
  operation `uncertain`, and the reconciler decides ([§6.3](#63-spend-intent-customer_reference-and-uncertain)).
  The exception is `send_sms`, whose Telnyx route has its own `Idempotency-Key`; see
  [§12.3](#123-dispatch-provider-idempotency-and-uncertain).
- **Fixed hosts.** The client calls only `https://api.telnyx.com/v2` or `https://api.telnyx.eu/v2`
  (`PM_TELNYX_API_BASE`, one of the two, checked at isolate start) and `https://api.vapi.ai`. No request field
  names a provider URL ([Security § 9.3](security.md#93-other-outbound-destinations)).
- **One place maps statuses.** `core::phone::telnyx` holds every mapping from a Telnyx enum to an internal
  state, with an exhaustive table test, and an unknown enum value maps to "unknown, keep the previous state
  and alert", never to a guessed state (a brand status such as `VETTED_VERIFIED` is mapped, not assumed, [§13](#13-us-a2p-10dlc-and-toll-free-verification)).

### 2.3 Telnyx behaviour this design relies on

Each item is re-checked by a named contract test ([§35](#35-tests)) before code depends on it.

| Behaviour | Source (read 2026-10-24) | Used in |
|---|---|---|
| `POST /number_orders` takes `phone_numbers[{phone_number, requirement_group_id}]`, `connection_id`, `messaging_profile_id`, `billing_group_id`, `customer_reference`, and has no idempotency key | OpenAPI `CreateNumberOrder` | [§6](#6-getting-a-number) |
| A number can be ordered only from a recent search (error 85000); a number taken meanwhile fails with 85001 | Telnyx error catalogue `/data/api-errors.json` | [§6.1](#61-search) |
| "Do not automatically retry a timed-out `POST` that can … purchase a resource … First reconcile" | Telnyx "Retries and reliability" | [§6.3](#63-spend-intent-customer_reference-and-uncertain) |
| Order and sub-order statuses `pending`, `success`, `failure` (the guide adds `cancelled`, `deleted`); per-number `requirements_status`; a `deadline` that cancels the order | OpenAPI; number orders guide; support articles ("10 days … otherwise the numbers will be removed") | [§6.5](#65-requirements-deadlines-and-long-reviews) |
| `POST /messages` takes an optional `Idempotency-Key` (1–255 of `[A-Za-z0-9_-]`); only successful responses are replayed, for 24 hours; same key in progress `409`; same key, other body `422`; malformed `400` (10015) | OpenAPI `SendMessage`, parameter `IdempotencyKey` | [§12.3](#123-dispatch-provider-idempotency-and-uncertain) |
| Message payloads carry `cost {amount, currency}`, `tags`, `from`, `to[].status` | OpenAPI `OutboundMessagePayload`, `InboundMessagePayload` | [§12.4](#124-delivery-receipts-and-the-sender-used), [§18](#18-money) |
| Webhooks are signed with Ed25519 over `{telnyx-timestamp}|{raw body}`, headers `telnyx-signature-ed25519` and `telnyx-timestamp`; the receiver applies its own replay window | Telnyx webhooks fundamentals | [§9.2](#92-verification) |
| Messaging webhooks need a `2xx` within 2 seconds; 3 attempts per URL then 3 to the failover URL; unordered, with duplicates | Telnyx messaging webhooks | [§9.3](#93-acknowledge-stage-queue) |
| Opt-out block rules apply to the whole messaging profile ("use separate messaging profiles") | Telnyx advanced opt-in/out | [§8](#8-tenant-provider-resources), [§10.5](#105-opt-out-keywords-and-the-consent-ledger) |
| Port-outs are authorised automatically if not answered within 24–48 hours; `external_pin` is checked against the gaining carrier's request | Telnyx port-out guide | [§16](#16-the-port-out-guard) |
| Documents must be linked within 30 minutes or are deleted; at most 20 MB; addresses are immutable | Telnyx documents and addresses guides | [§4.3](#43-requirement-groups-and-submission) |
| 10DLC campaign `status` is `ACTIVE`/`EXPIRED`, vetting progress is the separate `campaignStatus` (`TCR_PENDING` … `MNO_PROVISIONED`); brand `identityStatus` includes `VETTED_VERIFIED`; events `10dlc.brand.update`, `10dlc.campaign.update` go to the brand's or campaign's `webhookURL` | Telnyx 10DLC API reference and event notifications | [§13](#13-us-a2p-10dlc-and-toll-free-verification) |
| Messaging profiles take `whitelisted_destinations`, `daily_spend_limit`, `webhook_url`; outbound voice profiles take `whitelisted_destinations`, `daily_spend_limit`, `concurrent_call_limit`, `max_destination_rate`; credential connections take `user_name`, `password`, `webhook_event_url`, `call_cost_in_webhooks` | OpenAPI `CreateMessagingProfile`, `CreateOutboundVoiceProfile`, `CreateCredentialConnection` | [§8](#8-tenant-provider-resources) |
| `GET /webhook_deliveries` lists delivery attempts; porting and port-out events can be republished; `GET /detail_records` serves message and call records | OpenAPI | [§17](#17-reconciliation) |

### 2.4 Deployment set-up

`idvy setup phone` (a new CLI command, run once by the operator with a Telnyx key read from stdin) checks the
account level (a Verified-only operation answered with 10038 means not Verified), reads the webhook public key
from the operator's input (the portal shows it), uploads `PM_TELNYX_API_KEY` with `wrangler secret put`, writes
`PM_TELNYX_PUBLIC_KEY` (a public key, not a secret) into `deploy/wrangler.toml` as a variable, creates the account's notification profile, a webhook channel to
`https://{PM_API_HOST}/hooks/telnyx` and notification settings for number orders, requirement groups,
porting orders and port-outs (the number-order events need them: Telnyx number orders guide, read
2026-10-24), and prints what remains manual (data locality,
the reseller annex, the account level). It writes nothing else to the account: tenant resources are
created per tenant ([§8](#8-tenant-provider-resources)). `idvy doctor` re-checks every item, plus the
signature of a test webhook (`POST /hooks/telnyx` of a Telnyx test event).

### 2.5 US enablement

GB is in place; the US rows stay `eligible: false` (`platform_prerequisite`) until each step below is done,
in this order. Each step names its proof.

| Step | Who | Proof |
|---|---|---|
| 1. The Telnyx account at the Verified level, which orders numbers outside the account's own country and enables 10DLC and toll-free verification ([§2.1](#21-one-account-and-provider_account_id-everywhere)) | Owner (KYC of TREFT LTD and Telnyx's agent evaluation) | `idvy setup phone` records `level = "verified"` |
| 2. 10DLC as an ISV on the account: brand and campaign webhooks to `/hooks/telnyx`, every path under `/v2` ([§13](#13-us-a2p-10dlc-and-toll-free-verification)) | Build (M39) | `live::phone::a2p_paths` and `live::phone::a2p_events` pass on staging |
| 3. The US local row turned on, with each number reporting `outbound_sms` ([§6.4](#64-readiness-capabilities-and-pylotas-failure-codes)) | Operator (`PM_PHONE_CATALOGUE`, or the release that flips the row) | `GET /v1/phone/countries` lists US local as eligible for a ready tenant |
| 4. Emergency addresses for numbers that place calls ([§14.8](#148-emergency-calling)) | Build (M40) | `it::phone::us_emergency` and `live::phone::e911_address` |
| 5. One real US business onboarded end to end on staging (brand, campaign, a number, a text delivered), and one GB-registered business's US brand ([§13.2](#132-brands-registered-outside-the-us)) | Owner and build | `live::phone::us_onboarding_end_to_end`, `live::phone::uk_brand_registration` |
| 6. The standing budget and the one-call route ([§5.7](#57-the-standing-purchase-budget), [§6.7](#67-one-call-purchase)) | Build (M38) | `it::phone::buy_within_budget` |

Until step 5's second test passes and counsel has answered question 11, a US number bought by a tenant whose
profile is registered outside the US is sold for receiving and inbound calls only: its `outbound_sms` is
`not_available` with `reason = "brand_country_not_supported"`, and its quote says so.

## 3. The country catalogue

### 3.1 Rows and fields

What can be bought, where, with which capabilities and on which conditions is data, not code:
`crates/core/data/phone-catalogue.json`, compiled into the Worker and overridable with
`PM_PHONE_CATALOGUE` (validated at `idvy deploy` and at isolate start as the plan catalog is,
[Billing › Plan catalog](billing.md#plan-catalog)). One row per country and number type:

| Field | Meaning |
|---|---|
| `country`, `number_type` | ISO 3166-1 alpha-2 and one of `mobile`, `local` (geographic), `national`, `toll_free` |
| `status` | `available` (any business tenant whose profile passes review may buy it), `requires_local_presence` (offered only to a tenant whose compliance profile holds a verified address, and where the row says so an establishment, in the country; for `local`, an address in the number's area), `not_offered` |
| `capabilities` | `sms`, `voice`, `mms_inbound`; what the row promises after activation. A capability the provider has not shown for the type is absent until a contract test shows it |
| `sms_use` | `two_way`, `a2p_only` (for example Dutch `+3197` numbers, "A2A / A2P use only"), or `none` |
| `requirements` | `{action: [requirement keys]}` for `ordering` and `porting`: the profile fields and document kinds needed, mapped to the provider's requirement types at submission ([§4.3](#43-requirement-groups-and-submission)) |
| `requirement_group` | `required` (orders must carry an approved group: Ireland, Italy), `recommended` or `none` |
| `pre_approval` | Whether a group can be approved before ordering (Italy cannot) |
| `local_presence` | `none`, `address_in_country`, `address_in_area`, `physical_presence` |
| `sender_rules` | Long-code sending rules for SMS sent from the number: `us_a2p` (10DLC or toll-free verification), `none`; and per destination `sender_substitution_risk` notes |
| `destinations` | Per destination country, whether SMS from this row reaches it: `tested_ok` (a contract test delivered a text, with its date), `untested`, or `not_supported` (with the source). A send to an `untested` or `not_supported` destination is refused ([§12.2](#122-policy-pipeline) step 6) ([§3.5](#35-sending-across-borders)) |
| `default_for_country` | `true` on the row a purchase uses when it names only the country: GB mobile, US local ([§6.7](#67-one-call-purchase)) |
| `entity_types` | The profile entity types the row may be bought under (`business` only: sole traders are refused, [§1.1](#11-sole-traders-refused-owner-decision-2026-10-26)) |
| `registration` | For rows whose SMS needs a registration before sending (`us_a2p`): the fee items (rate-card keys) and `typical_wait_days` shown in quotes and the setup status ([§5.3](#53-quotes-and-the-digest)) |
| `quiet_hours` | For marketing SMS **to** the country: time zone, local start and end, days off, dated holidays per year (France: no marketing on Sundays, on public holidays or from 22:00 to 08:00; Telnyx France SMS guideline 6531603, read 2026-10-24) |
| `emergency` | `{numbers, outbound_supported, address_registration}` ([§14.8](#148-emergency-calling)) |
| `rate_card_key` | The rate card row ([§18.1](#181-prices-currency-and-the-rate-card)) |
| `review_hours` | Typical provider review time, shown to people (about 72 hours in GB, IE, DE, FR, ES, IT: Telnyx DID requirement articles) |
| `sources`, `verified_at` | The URLs the row rests on and the date they were last read; `cargo xtask phone-catalogue` fails a release whose rows were verified more than 180 days before |

`GET /v1/phone/countries` (any key with `phone:read`) returns the rows with `status` other than
`not_offered`, and for each the tenant's own standing: `eligible`, or the reason it is not
(`local_presence_required`, `compliance_missing`, `not_allowed_by_policy`).

### 3.2 The launch catalogue

The launch set follows the evidence of 2026-10-24. Ireland and the Netherlands are the EU countries
offered first: their numbers can be held by a business that is not established in a stricter way than an
address, and Pylota's next market is Ireland. Germany, France, Spain and Italy are offered only to a
tenant with the local presence their rules require, because each of their requirement articles says
"end-users must be physically present in the country".

| Country | Type | Status | Capabilities | Local presence | Notes (sources read 2026-10-24) |
|---|---|---|---|---|---|
| GB | mobile | available | sms, voice | none | Only GB mobile numbers carry SMS (Telnyx "Find GB numbers" 5820047). Business: representative, company, registration certificate, use case, an address with a utility bill under 3 months; "indicate if you are sub-allocating" (UK DID requirements 1311457) |
| GB | local | requires_local_presence | voice | address_in_country | Voice only. **Conflict**: the current Telnyx article accepts a worldwide address; an earlier version asked for one matching the area code, and Twilio's guideline asks for a UK address. The stricter reading holds until a contract test of an order shows otherwise |
| US | local | available | sms, voice, mms_inbound | none | No documents; SMS needs 10DLC ([§13](#13-us-a2p-10dlc-and-toll-free-verification)); an E911 address for numbers that make calls; Verified account level needed |
| US | toll_free | available | sms, voice | none | Optional. SMS only after toll-free verification; never chosen as a fallback for `local` |
| IE | local | requires_local_presence | voice | address_in_area | Requirement group mandatory; proof of address within 3 months (Ireland DID requirements 1311458) |
| IE | mobile | requires_local_presence | voice | address_in_country | Businesses only. Two-way SMS on Irish numbers is not documented (unknown 7.3.1 of the study): `sms` is absent until a contract test shows it |
| NL | mobile | available | sms | none | `+3197` range: A2P only, no person-to-person voice; address anywhere (Netherlands DID requirements 1311472; Twilio NL guideline) |
| NL | local | requires_local_presence | voice | address_in_area | Proof of address within 3 months |
| DE | local | requires_local_presence | voice | physical_presence | Signed German registration form; scanned documents only (Germany DID requirements 1311450) |
| FR | local | requires_local_presence | voice | physical_presence | French rules have barred sub-allocating geographic numbers since 2023-01-01 (Bandwidth regulations, read through search), so a French number is only ever registered in the tenant's own name ([§3.3](#33-rules-the-catalogue-enforces)) |
| FR | mobile | not_offered | | | French mobile numbers ban A2P messaging and automated calls (Twilio FR guideline) |
| ES | local | requires_local_presence | voice | physical_presence | DNI/NIE or CIF; address in the area (Spain DID requirements 1311073) |
| IT | local | requires_local_presence | voice | physical_presence | Requirement group mandatory since 2024-09-16, no pre-approval, PDF only; personal tax code of the representative (Italy DID requirements 1311462) |
| any other | any | not_offered | | | |

Rows `national` and GB `toll_free` are `not_offered` in v1: no tenant has asked for them, and each adds
another rule set to keep current.

### 3.3 Rules the catalogue enforces

- **Search and requests** for a `not_offered` row get `422 country_not_offered`; for a
  `requires_local_presence` row without the presence `422 local_presence_required` with
  `details.local_presence` ([P3](#34-edge-cases)).
- **No sub-allocation from TREFT's name.** Every order carries the requirement group of the tenant's own
  profile where the row has `requirement_group` other than `none`, and the profile's legal entity is the
  tenant. A purchase whose tenant has no approved profile for that row waits on onboarding
  (`blocked_by: ["compliance"]`, [§5.8](#58-requests-that-wait-on-onboarding)) and is never executed until
  the group is approved, or, for a row with `pre_approval: false` (Italy), until the group is **complete**: every
  requirement mapped and filled, and the group created at the provider, which reviews it only with an order. That
  order carries the group, the number stays `under_review` or `requirements_pending`, and it is never moved to
  `configuring` or `active` (no facet is configured) until the provider approves the group ([P130](#34-edge-cases)); other request kinds that need an approved profile (`port_in`) are refused with
  `409 compliance_not_approved`. Either way no number is ever activated under TREFT's identity
  ([P5](#34-edge-cases)). Pylota's platform-wide requirement group
  is not carried over except for numbers adopted during migration, each of which must be re-papered under
  its tenant's profile within 90 days ([§7.6](#76-adoption-for-pylotas-migration)).
- **Capabilities are promises.** A number's `capabilities` are the row's at purchase; a send through a
  capability the number lacks gets `409 phone_number_not_ready` with `details.reason = "capability"`.
- **Quiet hours** apply to marketing SMS by destination country, whatever the sender's country
  ([§12.2](#122-policy-pipeline) step 10).

### 3.4 Changing the catalogue

A row moves between statuses only by a release (or by the operator's `PM_PHONE_CATALOGUE`). Moving a row to
`not_offered` stops new searches and requests; it never touches numbers already held. A changed
`requirements` set applies to the next submission; a profile already approved for the row stays approved
until the provider says otherwise (a requirement group can become `no-longer-eligible`, [§4.4](#44-states-expiry-and-re-papering)).

### 3.5 Sending across borders

A tenant may hold numbers in several countries, and `phone.sms.allowed_destination_countries` lets it text
any of them. Whether a text from one country's long code reaches another country's handsets is not the
same question, and the evidence is thin:

- AgentPhone's API reference says its verification codes go "through carrier-registered sender IDs, so it
  reaches countries where plain application SMS from a US long code is not deliverable (e.g. Greece, UAE, the
  Philippines, the UK)" (npm `agentphone` 1.0.25, `reference.md`, Verification, read 2026-10-26). The US local
  row therefore marks GB `not_supported`.
- Whether US carriers filter A2P traffic from a GB mobile number to US handsets was not found in what was
  read. The GB mobile row marks US `untested`.

A row's `destinations` change only by a catalogue release that cites `live::phone::cross_border_sms`
(`gb_mobile_to_us`, `us_local_to_gb`), which sends a text each way on staging and records delivery receipts.
A same-country destination is always `tested_ok` once the row's own contract tests pass. The guide tells a
tenant that texts customers in two countries to hold a number in each.

## 4. Compliance profiles and KYC documents

### 4.1 The profile

One **compliance profile** per tenant (`phone_compliance_profiles`, `cpp_`): the legal entity in whose
name the tenant's numbers are registered. It is created and edited by a key holding `phone:compliance`
(platform, partner or tenant; never identity keys) or by a console owner or admin.

| Field | Rule |
|---|---|
| `legal_name`, `trading_name` | 1–200 characters |
| `entity_type` | `limited_company`, `llp`, `partnership`, `public_company`, `non_profit`; `sole_trader` is refused (owner decision, [§1.1](#11-sole-traders-refused-owner-decision-2026-10-26)). Individuals are refused |
| `registration_country`, `company_number` | ISO country; the registry number (Companies House for a UK company, accepted by Telnyx toll-free verification as `businessRegistrationType: "Companies House"`) |
| `company_registry` | `{country, region, name}`: the register that holds `company_number`. For a UK company `{GB, null, "Companies House"}`; for a US company the state, because US companies are registered with a state (`{US, "US-DE", "Delaware Division of Corporations"}`); for an Irish company `{IE, null, "Companies Registration Office"}`. `region` is an ISO 3166-2 code. Required when `registration_country` has more than one register |
| `tax_ids` | Per country: VAT, CIF, SIREN, Italian VAT, US EIN. **Sealed** (`tax_ids_sealed`) |
| `website`, `use_case` | The business use case text required by several countries; 1–1,000 characters |
| `representative` | Name, role, email and phone of the authorised representative; email and phone **sealed** (`representative_sealed`); some countries also need date and place of birth, nationality and a personal tax code (Italy), kept in the same sealed object and asked for only when a row needs them |
| `addresses[]` | Up to 20: `{id, kind (registered, service, establishment), lines, locality, region, postal_code, country, verified}`. An address is never edited in place: a change creates a new record, as the provider's addresses are immutable |
| `status` | Per (country, number type, action), from the requirement groups ([§4.4](#44-states-expiry-and-re-papering)) |

### 4.2 Documents

A KYC document is uploaded once and is **write-only**: no API route and no console page returns its
bytes. Uploading follows the upload pattern ([Uploads § 3](uploads.md#3-routes)) with stricter rules:

1. `POST /v1/tenants/{tenant_id}/phone-compliance/documents` (`phone:compliance`) with `kind`
   (`company_registration_certificate`, `proof_of_address`, `representative_id`, `registration_form`,
   `power_of_attorney`, `letter_of_authorisation`, `invoice`, `other`), `filename`, `content_type`
   (`application/pdf`, `image/png` or `image/jpeg`), `size` (1–20,971,520 bytes: Telnyx's 20 MB maximum)
   and `sha256`; optional `valid_until` (a proof of address is "within 3 months"). Answers `201` with the
   document (`cpd_`) and a signed `upload_url` (purpose `phone_document`, one hour).
2. `PUT /v1/links/{token}` streams the bytes into the Worker, which checks size and SHA-256, sniffs the
   type (`core::attach` rules: an executable, macro or archive is refused with `422 attachment_risky`),
   generates a 32-byte data key from `platform::Rng`, encrypts the whole document with AES-256-GCM (aad
   `pm1|phone_documents|{cpd_id}`), writes the ciphertext to R2 at `t/{ten}/phone/docs/{cpd}`, and seals the
   data key under the master key in `phone_compliance_documents.key_sealed` (aad
   `pm1|phone_compliance_documents|key_sealed|{cpd_id}`). The plaintext never reaches R2, a log or a queue.
3. The document is `ready`. It can be replaced (a new document) or deleted (`DELETE`, which deletes the
   object, then the sealed key; a document linked to a live provider requirement group is refused with
   `409 phone_document_in_use` until the group no longer needs it).

Deleting the sealed key makes the R2 ciphertext unreadable even if an object delete failed, so every
document erasure is a crypto-shred followed by an object delete, both counted in the receipt
([§24](#24-privacy)).

### 4.3 Requirement groups and submission

What the provider needs is data: `GET /requirements?filter[country_code]&filter[phone_number_type]&filter[action]`
returns versioned requirement types with acceptance criteria (Telnyx requirements API, read 2026-10-24). The
Worker caches the set per (country, type, action, version) for 24 hours in `phone_requirement_groups`
(`requirements_json`), and `core::phone::catalogue::map_requirements` maps each provider requirement type to
a profile field or a document kind. A requirement it cannot map stops submission with
`422 compliance_incomplete` and `details.missing[]` naming it, and raises `phone_requirement_unmapped` for
the operator ([P11](#34-edge-cases)).

The mapping depends on where the tenant is registered as well as on the number's country, because a
provider requirement names a kind of document, not the document a given country issues.
`core::phone::catalogue::document_for(requirement, registration_country, company_registry)` returns the
document kinds that satisfy a requirement and the wording shown to the person:

| Requirement (as the provider names it) | GB-registered | US-registered | IE-registered | Elsewhere |
|---|---|---|---|---|
| Company registration document | Companies House certificate of incorporation (`company_registration_certificate`) | The state's certificate of incorporation or formation, or the articles (`company_registration_certificate`, `company_registry.region` required) | CRO certificate of incorporation | `company_registration_certificate`, and the submission waits for the operator's review (`phone_requirement_unmapped`) |
| Proof of address under 3 months | A utility bill or bank statement (`proof_of_address`, `valid_until` set to its date + 3 months) | The same, for a US address | The same | The same |
| Business registration number for toll-free verification (`businessRegistrationType`) | `"Companies House"` (Telnyx toll-free verification guide, read 2026-10-24) | The value Telnyx accepts for a state registration, found by `live::phone::toll_free_registration_types` before US-registered tenants can submit | Found by the same test | Found by the same test |

A requirement mapped to `other` for a registration country is never guessed: the submission stops with
`422 compliance_incomplete` and `details.missing[]` until a catalogue release maps it.

Submitting a profile for a (country, number type, action) is a **phone request** of kind
`submit_compliance` that a person approves, because it sends personal data and identity documents to a
processor ([§5](#5-phone-requests-approval-for-everything-that-costs-money-or-cannot-be-undone)). On
execution, in one claimed run:

1. create the provider addresses the requirement set needs (`POST /addresses`, validated; a validation
   failure returns the provider's suggested address to the profile as `details.suggested`, and the request
   becomes `failed` with `compliance_address_invalid`; the address is never stored with
   `validate_address=false` for a number that will carry emergency calls, because a provider address cannot
   be fixed later: Telnyx addresses guide);
2. for each document: decrypt in memory, `POST /documents` (base64 body), and **in the same run** create or
   update the requirement group (`POST /requirement_groups` or `PATCH`) that links it, inside the 30-minute
   window. A run that cannot finish linking within 20 minutes of its first upload abandons the uploaded
   provider documents (deleting them when unlinked) and fails with `upstream_error`, so no document waits
   at the provider unlinked;
3. `POST /requirement_groups/{id}/submit_for_approval` where the row allows pre-approval; otherwise the
   group is attached to the next order;
4. store the provider IDs and statuses in `phone_requirement_groups` and the profile's status.

Each provider object carries the tenant's tag (`pi:t:{tenant_id}`), so the reconciler can attribute
it ([§17](#17-reconciliation)).

### 4.4 States, expiry and re-papering

| Provider state (requirement group) | Profile status for the row | Who acts |
|---|---|---|
| `unapproved` | `draft` | The tenant completes and submits |
| `pending-approval` | `under_review` | Nobody; the reconciler polls at 1 h, 6 h, then daily, and alerts the operator after 7 days |
| `approved` | `approved` | Orders for the row can proceed |
| `declined` | `rejected`, with the provider's comments as untrusted text | The tenant fixes and resubmits (a new request) |
| `expired`, `no-longer-eligible` (**Conflict**: absent from the OpenAPI enum) | `expired` | The tenant re-papers; numbers keep working until the provider says otherwise |
| An unknown value | unchanged, `phone_status_unknown` raised | The operator |

Requirement values carry `expires_at` at the provider; the profile copies the earliest one, and a
notification goes to the tenant's owners 30 and 7 days before it ([§27](#27-console-and-notifications)).
Whether the provider enforces expiry on active numbers is not documented (unknown 7.3.6 of the study), so
the design re-papers before expiry rather than waiting to find out.

### 4.5 Who sees what

| Data | Who can read it |
|---|---|
| Profile fields that are not sealed (names, company number, country, addresses, statuses) | `phone:compliance` keys, console owners and admins |
| Sealed fields (tax IDs, representative contact, birth data) | Nobody through the API or console after they are written; shown masked (`•••• 4821`). Opened only inside the executor, in memory, to submit to the provider |
| Document bytes | Nobody. The platform export of [§4.6](#46-retention-and-erasure) is the one exception |
| Document metadata (kind, filename, size, status, `valid_until`) | `phone:compliance` keys, console owners and admins |
| Provider comments on a rejection | As profile fields; escaped as untrusted text |

Identity keys and agents never read any of it: a compliance route answers an identity key
`403 scope_denied`.

### 4.6 Retention and erasure

- **While needed.** A profile and its documents are kept while any number of the tenant uses a
  requirement group built from them, and while a request that needs them is open.
- **After the last number.** When the tenant's last number using the profile is released or ported out,
  the documents are kept for `PM_PHONE_KYC_RETENTION_DAYS` (default 365), because the reseller terms make
  TREFT keep each end user's records "for the locally mandated retention period" and give them "to Telnyx
  or authorities upon request" (Telnyx country-specific terms, reseller requirements annex, read
  2026-10-24). The default is a placeholder until counsel sets a period per country
  ([§32](#32-questions-for-counsel), question 6); then the retention job deletes them with an audit row.
- **Tenant erasure** deletes the profile and every document, except documents still inside their
  retention period, which are kept under the hold reason `kyc_retention` with their `retain_until`, and
  listed in the receipt as `phone_documents_retained`. Whether that hold is lawful against an erasure
  request is question 6 for counsel; until counsel answers, the hold applies only to documents of numbers
  released within the last `PM_PHONE_KYC_RETENTION_DAYS`.
- **Provider copies.** After the last number is released, the executor unlinks and deletes the tenant's
  provider documents and requirement groups (`DELETE /documents/{id}`, `DELETE /requirement_groups/{id}`),
  counting `provider_deletions` in the receipt; a provider copy that cannot be deleted (still linked by the
  provider) is retried daily for 30 days, then reported to the operator.
- **Regulator requests.** `POST /v1/platform/phone-compliance/{profile_id}/export` (`platform:ops`) builds an
  encrypted ZIP of the profile and its documents, behind a single-use signed link valid for one hour, with an
  audit row naming the operator and the reason (1–500 characters). It is the only path by which document
  bytes leave the Worker other than submission to the provider.

### 4.7 Phone onboarding

The regulatory work a country needs is done **once per tenant and country**, not once per number. Phone
onboarding is the guided view of that work: the profile, the documents, the requests a person approves, the
provider's reviews and the platform's own prerequisites, as one status per country, so that an agent always
knows what is missing and who must act.

| Country | Once per tenant | Then, for every number |
|---|---|---|
| GB | A business profile and its documents; one `submit_compliance` for GB mobile ordering, approved by a person; the provider's review (about 72 hours, [§3.2](#32-the-launch-catalogue)) | Every GB mobile order carries the tenant's approved requirement group |
| US | A profile (US local needs no documents to order); for texting, one `register_us_a2p` (brand and campaign) approved by a person, and carrier vetting; for calls, an emergency address per calling number | Every US number joins the tenant's campaign at configuration ([§13](#13-us-a2p-10dlc-and-toll-free-verification)) |

**The status.** `GET /v1/tenants/{tenant_id}/phone-setup` (`phone:read`, every key level: an identity key may
read it, because it holds no personal data) returns one entry per catalogue country the tenant's policy
allows:

```json
{ "country": "US", "status": "action_needed",
  "steps": [
    { "kind": "platform_prerequisite", "status": "done" },
    { "kind": "profile", "status": "done" },
    { "kind": "register_us_a2p", "status": "not_started", "who": "person",
      "fee": { "setup_pence": 2839, "monthly_pence": 823 }, "typical_wait_days": [7, 15] },
    { "kind": "enable_emergency", "status": "optional", "who": "person", "applies_to": "numbers that place calls" } ],
  "after_setup": { "outbound_sms": "enabled", "inbound_sms": "enabled", "inbound_calls": "enabled",
                   "outbound_calls": "registration_required" },
  "next_step": { "who": "person", "kind": "register_us_a2p",
                 "console_url": "https://console.example.com/phone/setup/US", "draft_id": "pdr_01JB…" } }
```

| `status` | Meaning |
|---|---|
| `not_started` | Nothing submitted |
| `in_review` | Everything a person must do is done; the provider or the carriers are reviewing |
| `action_needed` | A person must act (fill a field, upload a document, approve a request, answer a rejection); `next_step` names it |
| `ready` | A purchase for the country's default row can execute, and every capability that needs a registration has it |
| `unavailable` | The platform cannot offer the country to this tenant (`platform_prerequisite`, `local_presence_required`, `entity_type`, or `brand_country_not_supported` for US texting) with `reason` |

`steps[].kind` is one of `platform_prerequisite`, `profile`, `documents`, `submit_compliance`,
`register_us_a2p`, `verify_toll_free` and `enable_emergency`; each step's `status` is `not_started`,
`draft`, `waiting_for_person`, `in_review`, `done`, `rejected` (with the provider's comments as escaped
text), `optional` or `not_applicable`. `typical_wait_days` comes from the catalogue row's `registration`
([§3.1](#31-rows-and-fields)); the only public figure read for US 10DLC is AgentPhone's "Carrier approval
typically takes 7-10 business days" (npm `agentphone` 1.0.25, `reference.md`, Registration, read 2026-10-26),
which the catalogue widens to 7–15 calendar days until `live::phone::us_onboarding_end_to_end` measures
it. `core::phone::onboarding::status(facts) -> SetupStatus` is pure; the handler loads the profile, its group
states, the registrations and `platform_state.phone_account`. The MCP tool `phone_setup_status` returns the
same object.

**Agents draft; people submit.** An agent knows what the business does and how it talks to its customers,
so it may draft the text a registration needs: `PUT /v1/tenants/{tenant_id}/phone-setup/{country}/draft`
(`phone:request`, every key level, `Idempotency-Key` optional) stores a **draft** (`phone_onboarding_drafts`,
`pdr_`) with the use-case description, the business description, 2–5 sample messages (each 20–1,024
characters and containing the brand name, the rule AgentPhone's registration states for US campaigns: npm
`agentphone` 1.0.25, `RegistrationRequest`, read 2026-10-26), the opt-in consent description, the website,
the privacy policy and terms URLs. A draft never holds a tax identifier, a representative's personal data or a
document, and a field outside its schema is refused (`400 invalid_request`). Drafts are untrusted text,
shown escaped, with `drafted_by_key_id`. A person opens the console's **Phone › Setup** page, sees the
draft beside the profile, edits it, attaches documents and submits, which creates the `submit_compliance` or
`register_us_a2p` request and approves it in the same step (the person who submits is the person who decides,
after re-authentication). A draft is deleted when its request is created, and after 30 days untouched.

**Prefill.** An integrator that already holds a tenant's business details (Pylota holds its operators')
writes them with `PATCH …/phone-compliance` and the draft route through its partner key, so most people see a
confirmation screen rather than a blank form.

## 5. Phone requests: approval for everything that costs money or cannot be undone

### 5.1 Kinds

A **phone request** (`phone_requests`, `phr_`) is the object a person approves. It reuses the approval
machinery of drafts and the service sign-up ledger: the decision goes through
`auth::people::key_may_decide` ([Workspace policy § 3](workspace-policy.md#3-decisions-reserved-for-people)),
a key never approves its own request, a partner key can record `on_behalf_of` for the person in its own
application, undecided requests expire after 7 days, and the tenant's queue appears in the console's
"Needs a person". Only the object is new, as each approvable thing (a draft, a ledger entry) has its own.

| Kind | Created by | What approval lets happen | Billable at the provider |
|---|---|---|---|
| `purchase_number` | `phone:request` (any key level, agents included), directly or through the one-call route ([§6.7](#67-one-call-purchase)) | Order one number (exact, or any number matching criteria at or under a price) and configure it; with `expires_at`, also release it then ([§6.8](#68-throwaway-numbers)). Inside the tenant's standing budget the request is created already approved ([§5.7](#57-the-standing-purchase-budget)) | Yes: one-time and monthly charges |
| `port_in` | `phone:request` with `phone:compliance` for the losing-carrier details | Submit porting orders for the listed numbers | Provider charges only for changes, rejections and expedites ([§15](#15-porting-in)) |
| `release_number` | `phone:request` | Remove the voice route and delete the number at the provider | No; it cannot be undone |
| `submit_compliance` | `phone:compliance` | Send the profile and documents to the provider for a (country, type, action) | No; it moves personal data to a processor |
| `register_us_a2p` | `phone:compliance` | Register or change a 10DLC brand, campaign, vetting or number assignment | Yes: non-refundable fees ([§13](#13-us-a2p-10dlc-and-toll-free-verification)) |
| `verify_toll_free` | `phone:compliance` | Submit a toll-free verification | No fee is published; it moves business data to a processor |
| `enable_emergency` | `phone:manage` | Register an emergency address on a number | Yes: US E911 is $1.50 a number a month (Telnyx pricing, read 2026-10-24) |
| `set_voice_route` | `phone:manage` | Change where a number's calls go ([§14](#14-voice)) | A `forward` route incurs outbound minutes |
| `outbound_call` | `phone:call` | Place one call, when `phone.calls.outbound` is `approval` ([§14.5](#145-outbound-calls)) | Yes: per minute |

An agent's identity key can create `purchase_number`, `port_in` (without losing-carrier secrets, which a
tenant key adds), `release_number` and `outbound_call` requests for its own identity. It can never approve
one, and it cannot create the kinds that need `phone:compliance` or `phone:manage`, which identity keys
cannot hold ([§20](#20-permissions-and-key-levels)).

### 5.2 The request and its states

```text
                    approve                claim
pending_approval ───────────▶ approved ───────────▶ executing ──▶ completed
   │  ▲                          │                     │  ├──▶ in_progress ──▶ completed | failed | action_needed
   │  └── quote_changed ─────────┼─────────────────────┘  ├──▶ action_needed ──(person fixes, new submission)──▶ executing
   ├── reject ──▶ rejected       └── cancel ──▶ canceled   ├──▶ uncertain ──reconcile──▶ completed | in_progress | approved (absent)
   ├── 7 days ──▶ rejected (expired)                       └──▶ failed
   └── cancel ──▶ canceled
```

| Field | Meaning |
|---|---|
| `id`, `tenant_id`, `identity_id` | From the path and the key's scope, never the body |
| `kind`, `payload` | The kind and its normalised body (`core::phone::requests::normalise`) |
| `quote` | The prices the approver sees, in GBP pence, with `valid_until` ([§5.3](#53-quotes-and-the-digest)) |
| `digest` | 64 hex characters binding `kind`, `identity_id`, `payload` and `quote` |
| `status` | As above |
| `reason` | For `rejected` (`operator`, `expired`), `failed` (a code of [§6.4](#64-readiness-capabilities-and-pylotas-failure-codes) or of [§22](#22-failure-handling)), `canceled` (`requester`, `identity_deleted`, `tenant_erasing`) |
| `requested_by_key_id`, `requested_by_user_id` | Who asked |
| `decided_by_key_id`, `decided_by_user_id`, `on_behalf_of`, `note`, `decided_at` | Who decided, for whom, why |
| `attempt`, `customer_reference`, `spend_intent_at` | The current execution attempt ([§6.3](#63-spend-intent-customer_reference-and-uncertain)) |
| `exec_token`, `exec_until` | The execution lease ([§5.5](#55-execution-leases-and-attempts)) |
| `next_check_at`, `checks` | When the reconciler looks next, and how often it has |
| `blocked_by` | For `purchase_number`: the onboarding steps the purchase waits on (`compliance`, `platform_prerequisite`), empty when none. The executor never claims a blocked request ([§5.8](#58-requests-that-wait-on-onboarding)) |
| `approved_by` | `person`, `key` (under `phone.key_approval`), `standing_budget` (with `budget_version`) or `purchase` (a release scheduled by an approved purchase) |
| `release_at` | For `purchase_number` with `expires_at`: when the scheduled release runs ([§6.8](#68-throwaway-numbers)) |
| `result` | IDs created (`number_id`, `port_in_ids`, `registration_id`), never provider IDs for tenant and identity keys |
| `expires_at` | While `pending_approval`: `created_at` + 7 days |

Requests are created with `POST /v1/identities/{identity_id}/phone-requests` (`Idempotency-Key`
**required**, because a request leads to a charge), listed per identity and per tenant (the tenant's queue),
read, approved, rejected and canceled ([§28](#28-mcp-and-cli)). A tenant has at most 20 requests in
`pending_approval` and 10 in `executing`, `in_progress`, `action_needed` or `uncertain`
(`422 phone_request_limit_reached`).

### 5.3 Quotes and the digest

Every request that leads to a charge carries a **quote** computed from the rate card at creation
([§18.1](#181-prices-currency-and-the-rate-card)):

```json
{ "currency": "GBP", "setup_pence": 231, "monthly_pence": 231, "usage": "sms_out_gb_pence: 7 per segment",
  "rate_card_version": 3, "valid_until": "2026-10-31T10:00:00Z" }
```

For `purchase_number` with `selection.mode = "any"`, the quote is a ceiling (`max_setup_pence`,
`max_monthly_pence`). The quote is valid for 7 days, the life of a pending request.

**What comes next is in the quote.** A number is not always usable for everything its row promises the
moment it is bought: a US number texts only after the tenant's 10DLC registration, and places calls only
after an emergency address. So a `purchase_number` quote carries `follow_on[]`, one entry per capability that
will still need a registration for this tenant when the number arrives:

```json
"follow_on": [
  { "capability": "outbound_sms", "kind": "register_us_a2p", "setup_pence": 2839, "monthly_pence": 823,
    "typical_wait_days": [7, 15], "who": "person", "once_per": "tenant" },
  { "capability": "outbound_calls", "kind": "enable_emergency", "monthly_pence": 173, "who": "person",
    "once_per": "number" } ]
```

An entry is left out when the tenant has already done it (its registration is `ready`), and an entry whose
capability cannot be had at all for this tenant says so (`"kind": "not_available", "reason":
"brand_country_not_supported"`). The approval page and the one-call route's answer show `follow_on` beside
the price, and the digest covers it, so a person who approves a US number has seen what making it text costs.

**Expiry is in the quote.** A purchase with `expires_at` shows the setup price, the monthly price of every
month started before `expires_at` (months are charged whole, [§18.2](#182-what-is-charged)), and the
scheduled release. `digest` is
`hex(sha256(canonical_json({kind, identity_id, payload, quote})))` (`core::phone::requests::digest`), as a
draft's digest is ([Drafts § 2.4](drafts.md#24-revision-etag-and-digest)). An approval must name the
current digest. If execution finds a higher price than the quote (the rate card changed, or the provider's
cost for the chosen number exceeds the row's ceiling), the request returns to `pending_approval` with a new
quote, a new digest and `reason: "quote_changed"`, and the earlier approval no longer applies
([P20](#34-edge-cases)). A lower price is charged at the lower price.

Provider costs in USD and the exchange rate are in the quote only for platform and partner keys
(`quote.provider`); tenant and identity keys see GBP only.

### 5.4 Approval

`POST /v1/identities/{identity_id}/phone-requests/{request_id}/approve` with
`{ "digest": "…", "note": "…", "on_behalf_of": "usr_8812" }` (`phone:approve`; `Idempotency-Key`
required). Rules, in order:

1. **Permission.** `phone:approve`, which identity keys cannot hold.
2. **Who.** Approving lets money be spent, personal data leave, or a number go. It is a decision reserved
   for people, implemented by one new arm of `key_may_decide`: `Decision::ApprovePhoneRequest`, true for an
   API key when `PM_QUARANTINE_KEY_RELEASE` is `on` or `PM_CONSOLE` is `off`, or, where the tenant's policy
   has `phone.key_approval: true`, when the key is a platform key or the partner key of the tenant's own
   partner, never a tenant key ([J16](../edge-cases.md) rule). Otherwise `403 permission_denied` with
   `details.reason = "person_required"` ([P16](#34-edge-cases)). Pylota's operators' tenants have
   `phone.key_approval: true` set by Pylota's partner key, so Pylota approves from its own review screen with
   `on_behalf_of` naming the operator user who tapped approve.
3. **Not your own.** A key never approves a request it created: `403 permission_denied`,
   `details.reason = "self_approval"` ([P17](#34-edge-cases)).
4. **State and binding.** Only `pending_approval`; otherwise `409 phone_request_not_pending` with
   `details.status`. A digest that is not the current one: `409 phone_request_digest_mismatch` with
   `details.current_digest` ([P18](#34-edge-cases)). An expired request is set `rejected` (`expired`) and the
   call gets `409 phone_request_not_pending`.
5. **Commit.** One D1 batch, guarded by `status = 'pending_approval' AND digest = ?`: `status = approved`,
   the decision fields, the audit row `phone_request.approve` and the event `phone_request.approved`. The
   handler then queues `Execute { request_id }` on `pm-phone` (the every-minute cron also picks up any
   `approved` request older than one minute, so a lost queue send delays and never loses an approval).
6. **Respond** `200` with the request (`status: "approved"`).

Rejecting (`…/reject`, `{ "reason": "…" }`, 1–500 characters) is always allowed for a key with
`phone:approve`: it can only stop a spend. Canceling (`…/cancel`) is allowed to the requester and to
`phone:approve` keys while the request is `pending_approval` or `approved`.

### 5.5 Execution, leases and attempts

The `pm-phone` consumer runs `Execute { request_id }` ([§9.5](#95-the-pm-phone-consumer)). It **claims**
the request with one guarded statement, so two consumers, a cron sweep and a redelivered message can never
both run it:

```sql
UPDATE phone_requests SET status = 'executing', exec_token = ?tok, exec_until = ?now + 300000, updated_at = ?now
WHERE id = ?1 AND status IN ('approved','executing','in_progress','action_needed','uncertain')
  AND (exec_until IS NULL OR exec_until < ?now)
  AND EXISTS (SELECT 1 FROM tenants WHERE id = phone_requests.tenant_id AND status = 'active')
  AND EXISTS (SELECT 1 FROM identities WHERE id = phone_requests.identity_id AND status IN ('active','paused'));
```

`executing` is in the list so that a run whose Worker died is taken over once its lease lapses (the
`exec_until < ?now` guard keeps a live run safe). A run that takes over an `executing` request resumes from the
stored state, and when the current attempt has `spend_intent_at` set with no recorded provider outcome it first
treats that attempt as `uncertain` and reconciles it by `customer_reference` ([§6.3](#63-spend-intent-customer_reference-and-uncertain))
before any provider call, so a lost run never orders twice ([P129](#34-edge-cases)).

Every later write of the run is guarded by `exec_token = ?tok`, and a run that takes longer extends
`exec_until` by 5 minutes once a minute. A run whose guard fails stops at once, because another run or an
erasure has taken over (fencing). A tenant that started erasing, or an identity that started deleting, can
never get a new order: the claim fails, and the request is set `canceled` (`tenant_erasing`,
`identity_deleted`) by the erasure step ([§7.3](#73-identity-and-tenant-states)).

Before any billable call the run re-checks, in order: the ops switch `phone` allows the request's kind
([§30](#30-rollout-telemetry-and-rollback)): at `on` every kind; at `paused_purchases` only `outbound_call` and
`release_number` (purchases, ports, A2P and toll-free submissions, emergency registrations and voice-route
changes wait, and run when the switch returns to `on`); at `paused` only `release_number` ([P131](#34-edge-cases)); `PM_TELNYX_API_KEY` is set (otherwise the request fails with
`not_configured`, which is not retried, because no attempt can succeed until the operator acts); the
tenant's policy still allows the action; the compliance status the action needs is `approved`; the
tenant's provider resources exist and read back as expected ([§8](#8-tenant-provider-resources)), so a broken
webhook or profile is found before money is spent (Pylota parity: fail closed before spend); and the spend
hold is granted ([§18.3](#183-spend-caps)).

### 5.6 Expiry, events and audit

The every-minute cron sets `pending_approval` requests past `expires_at` to `rejected` (`expired`) with
`phone_request.rejected`. Every transition writes an audit row (`phone_request.{create, approve, reject,
cancel, complete, fail}`) and a platform event ([§25](#25-events-and-audit)) in the same D1 batch as the
change, with `tenant_id` and `identity_id` set and `sequence` null, as the service ledger's events are
([Service sign-up ledger § 6](service-accounts.md#6-events)).

### 5.7 The standing purchase budget

**This amends an owner rule; the owner approved it on 2026-10-26.** The rule
"a person approves every purchase" becomes "a person approves every purchase, one by one or through a
standing budget a person sets" ([ADR 0020](../adr/0020-phone-numbers.md), decision 12). Without a standing
budget a person stands in every purchase's path, and buying a number cannot be one call. Every other kind of
request (port, release, KYC, registration fees, routes, emergency addresses, calls) keeps per-request
approval.

`phone.auto_approve` is a tenant policy object of the **guard** class:

```json
{ "enabled": true, "countries": ["GB", "US"], "number_types": ["mobile", "local"],
  "max_numbers_per_identity": 2, "max_new_numbers_per_month": 10,
  "max_setup_pence_per_number": 300, "max_monthly_pence_per_number": 300,
  "expiring_only": false, "version": 4, "set_by": { "user_id": "usr_…", "key_id": null, "on_behalf_of": null },
  "set_at": "2026-10-26T09:00:00Z" }
```

| Field | Rule |
|---|---|
| `enabled` | Default `false` on every deployment and every tenant |
| `countries`, `number_types` | Subsets of the tenant's `phone.countries` and the catalogue's offered types |
| `max_numbers_per_identity` | 1–20; at most `phone.max_numbers_per_identity` |
| `max_new_numbers_per_month` | 1–200 per UTC month for the tenant, counted in `TenantQuota` (`phone_budget:t:{YYYY-MM}`) with a hold taken in the same transaction as the spend hold, so two racing purchases cannot both pass the last slot |
| `max_setup_pence_per_number`, `max_monthly_pence_per_number` | Ceilings on the quote; a candidate above them is outside the budget |
| `expiring_only` | When `true`, only purchases with `expires_at` are covered |

**Who sets it.** Setting it, or loosening any field, is a decision reserved for people
(`Decision::SetPhoneBudget`, one new arm of `key_may_decide`): a signed-in owner or admin in the console after
re-authentication, or a platform key or the tenant's partner key under `phone.key_approval` with
`on_behalf_of` (Pylota sets its operators' budgets from its own app). Tightening any field, or `enabled:
false`, is allowed to any key holding `phone:manage`. Identity keys never change it. `PUT
/v1/tenants/{tenant_id}/phone-budget` replaces the object (`Idempotency-Key` required); `GET` reads it with this
month's use. Every change bumps `version` and writes the audit row `phone_budget.set` and the event
`phone_budget.changed`.

**What it covers.** At creation, `core::phone::budget::covers(budget, request, counts) -> Result<(), Outside>`
decides, purely: the budget is `enabled`; the kind is `purchase_number`; the row's country and type are in the
budget; the identity's live numbers plus this one fit `max_numbers_per_identity`; the month's count has room;
the quote (or, for `selection.mode = "any"`, its ceiling) is within both price ceilings; `expires_at` is set when
`expiring_only`; and the request carries no `follow_on` entry of kind `not_available` that the caller did not
acknowledge (`acknowledge_not_available: true`). When it covers, the request is created `approved` with
`approved_by = "standing_budget"` and `budget_version`, in one D1 batch with the audit row
`phone_request.approve` (`details.standing_budget = version`) and the event `phone_request.approved`, and
executes at once. When it does not, the request is created `pending_approval` and `details.outside_budget`
names the first rule that failed, so the agent can tell a person why it is asking.

The spend caps of [§18.3](#183-spend-caps) still apply to a covered purchase, the ops switch and
`phone.allowed` still stop it, and a later quote change at execution returns the request to
`pending_approval` with the budget no longer applying ([§5.3](#53-quotes-and-the-digest)). The budget is not a
key's approval of its own request: it is a person's earlier decision, recorded with its version.

### 5.8 Requests that wait on onboarding

A `purchase_number` for a row the tenant has not finished onboarding (no approved group for the row, or a
platform prerequisite missing) is not refused. It is created with `blocked_by` naming what it waits on, its
status as the budget or a person decides, and the answer's `next_step` pointing at the onboarding step
([§4.7](#47-phone-onboarding)), so the agent has one thing to hand a person instead of a dead end. The
executor never claims a request whose `blocked_by` is not empty. When onboarding completes (a group becomes
`approved`, or complete for a row with `pre_approval: false` as in [§3.3](#33-rules-the-catalogue-enforces), the
prerequisite is recorded), the same D1 batch clears `blocked_by` on the tenant's waiting
requests for that row and, for those already approved, queues `Execute`. A blocked request's 7-day expiry
waits too: it expires 7 days after it is unblocked, or 30 days after creation if it never is, whichever is
sooner. A blocked request counts against the tenant's 20 pending requests. The refusals that remain refusals
are the ones no onboarding can fix: `422 country_not_offered`, `422 local_presence_required` and
`422 compliance_incomplete` with `details.reason = "entity_type"`.

## 6. Getting a number

### 6.1 Search

`GET /v1/identities/{identity_id}/phone-numbers/available` (`phone:request`) with `country` and `type`
(required), and `contains`, `starts_with`, `ends_with` (digits only), `locality`, `region` and `limit`
(1–20, default 10). The handler:

1. checks the catalogue row and the tenant's eligibility ([§3.3](#33-rules-the-catalogue-enforces)) and the
   tenant's policy (`phone.allowed`, `phone.countries`);
2. counts the search against the tenant's daily cap (300; `429 daily_cap_reached`, `details.cap =
   "phone_search"`) and the key's `search` rate-limit bucket;
3. calls the provider with the row's capabilities as required features (a GB mobile search asks for `sms`
   and `voice`, so a number lacking either is never offered: Pylota parity),
   `filter[exclude_held_numbers]=true`, and the row's number type only (never a fallback type);
4. drops candidates whose provider monthly or one-time cost exceeds the row's `max_provider_cost_usd`
   (vanity and premium numbers);
5. returns each candidate's `phone_number`, `locality`, `region`, `features`, its GBP `quote`, and a
   `quote_token`: a token signed with the link key (purpose `phone_quote`, [Security § 7.3](security.md#73-signed-links))
   over the number, the row, the quote and an expiry of 30 minutes. A `purchase_number` request with
   `selection.mode = "exact"` must carry a valid token; the request then holds the quote for 7 days.

Search results are not reserved: a provider reservation lasts 30 minutes (extensions add 30 minutes, with a
limit, errors 85007 and 85008), and a person may approve hours later. The executor searches for the exact
number again immediately before ordering (Telnyx refuses an order for a number not returned by a recent
search, error 85000).

### 6.2 The provisioning state machine

A number row (`phone_numbers`, `pnm_`) is created by the executor at the spend intent, so the number's
E.164 is claimed in the directory before any money is spent: the unique partial index on the live E.164
makes a second tenant's order for the same number fail cleanly before it reaches the provider
([P23](#34-edge-cases)).

```text
 Execute (approved)
      │ spend intent
      ▼
  ordering ──definitive refusal (proved no order)──▶ failed
      │  └──unknown outcome──▶ uncertain ──found──▶ (state of the found order)
      │                           └──absent, proved──▶ failed (attempt) ─▶ request approved, next attempt (≤ 3)
      │ order accepted
      ▼
  under_review ◀──▶ requirements_pending ──deadline passed, cancelled, failure──▶ failed
      │ carrier active (order success, number active)
      ▼
  configuring ──messaging and voice facets ready──▶ active
      │                                               ├──provider: not in service──▶ suspended ──▶ active
      │                                               ├──port-out pending──▶ port_out_pending ──▶ active | ported_out
      │                                               └──release approved──▶ releasing ──▶ released
  porting_in ──ported──▶ configuring        (port-in path, §15)
```

| Status | Meaning | Mapped from (Telnyx) |
|---|---|---|
| `ordering` | Spend intent written; the order call is about to run or ran without an answer yet | – |
| `uncertain` | The order call's outcome is unknown | A timeout, a lost connection or a `5xx` on `POST /number_orders` |
| `under_review` | The provider is vetting requirements | sub-order `pending` with `requirements_status = requirement-info-under-review` |
| `requirements_pending` | The provider needs information or rejected it; a person must act before `deadline_at` | `requirement-info-pending`, `requirement-info-exception` |
| `configuring` | Carrier active; messaging or voice not yet confirmed | order `success` and phone number `active` |
| `active` | Carrier active and every capability the number has is configured and read back | – |
| `suspended` | The provider reports the number out of service, or the platform suspended it | phone number `emergency-only`, or missing at the provider (the reconciler) |
| `port_out_pending` | A port-out request is open; the number still works | port-out `pending`, `authorized` before completion |
| `porting_in` | A port-in order is in progress | porting order statuses before `ported` |
| `releasing` | Release approved; route removal and provider deletion in progress | – |
| `released`, `ported_out`, `failed` | Final. The row stays as history ([§7.1](#71-the-directory-and-history)) | `deleted`; port-out `ported`; order `failure`, `cancelled`, `deleted`, deadline passed |

Transitions are a table in `core::phone::numbers::transition(state, event) -> Result<state, Refused>`, and
every provider status, including ones Telnyx's own pages disagree on (`cancelled` and `deleted` appear in
the guide but not in the OpenAPI enum), maps through `core::phone::telnyx`. An unknown value keeps the state
and raises `phone_status_unknown`.

### 6.3 Spend intent, `customer_reference` and `uncertain`

Telnyx's order route has no idempotency key, and Telnyx says not to retry a timed-out purchase before
reconciling. So every order attempt is bracketed by Pylota Identity's own records:

1. **Spend intent.** In one D1 batch guarded by the execution lease: `attempt = attempt + 1`,
   `customer_reference = "pi:" + request_id + ":" + attempt`, `spend_intent_at = now`, the number row in
   `ordering`, and the audit row. The reference is attempt-scoped, so recovery is exact (a reference shared
   by every attempt, such as the tenant ID, cannot tell one attempt from the next). Telnyx's
   length limit for `customer_reference` is not documented; the reference is at most 36 characters, and a
   contract test checks the provider accepts and filters on it.
2. **Call.** `POST /number_orders` with the number, its requirement group, the tenant's SIP connection,
   messaging profile and billing group, and the reference.
3. **Outcome.**
   - **Accepted** (`2xx`): store the provider order and sub-order IDs; the number moves by the order's
     state; `phone_request` becomes `in_progress`.
   - **Definitive refusal** (`4xx` with a provider error, or a `402`): before clearing anything, list orders
     by the reference. An empty, complete listing proves no order exists: the spend intent is cleared, the
     hold released, the number row `failed` with its code ([§6.4](#64-readiness-capabilities-and-pylotas-failure-codes))
     and the request `failed`. A listing that finds an order adopts it as if accepted. A listing that is
     incomplete or fails leaves the attempt `uncertain` (never a guess) ([P25](#34-edge-cases)).
   - **Unknown** (timeout, lost connection, `5xx`): the number and the request become `uncertain`; the hold
     is extended; `phone_request.uncertain` and `phone_number.status_changed` are emitted.
4. **Reconciling an uncertain attempt.** The reconciler lists orders by the reference at 5, 15, 30 and 60
   minutes, then hourly for 24 hours. A found order is adopted. Absence is **proved** only by two complete,
   empty listings, the first at least 5 minutes after the attempt and the second at least 10 minutes after
   the first (`core::phone::orders::absence_proved`). Then the attempt is over: the number row is `failed`,
   the request returns to `approved` and the executor makes the next attempt with a new reference, up to 3
   attempts; the approval covers exactly this purchase, and the earlier attempt provably did not happen.
   After 24 hours without proof either way, the request stays `uncertain` with `phone_order_uncertain_stuck`
   raised, and only `POST /v1/platform/phone-requests/{request_id}/resolve` (`platform:ops`, with
   `outcome: "ordered"` and the provider order ID, or `outcome: "not_ordered"`) ends it, after the operator
   has checked the provider portal ([P27](#34-edge-cases)). The lag between an order's creation and its
   appearance in the reference listing is not documented; `live::phone::order_listing_lag` measures it, and
   the 5-minute floor is raised before release if it is longer.

No path ever re-orders while an earlier attempt of the same request might exist: a new attempt needs the
previous one proved absent or adopted. Two requests of the same tenant for different numbers have
different references, so one's recovery never adopts the other's order (a duplicate-order refusal
becomes unnecessary and is kept as a check: two live orders for one
reference raise `phone_order_duplicate` and stop the request for the operator).

### 6.4 Readiness, capabilities and Pylota's failure codes

Three facets are read back from the provider and stored separately (Pylota parity 3):

| Facet | Values | Ready when |
|---|---|---|
| `carrier_status` | `pending`, `active`, `failed` | The order succeeded and `GET /phone_numbers/{id}` says `active` |
| `sms_status` | `not_applicable`, `pending`, `configuring`, `ready`, `failed` | `PATCH /phone_numbers/{id}/messaging` set the tenant's messaging profile and a read-back returned the same profile |
| `voice_status` | `not_applicable`, `pending`, `configuring`, `ready`, `failed` | The voice route of [§14](#14-voice) was created and read back |

**What the number can do.** The facets say whether configuration finished; they do not say whether an
agent can text from the number now, because a US number also needs the tenant's 10DLC registration and a
calling number an emergency address. So every number carries four **capability facets**, computed by
`core::phone::capabilities::of(row, facets, registrations, emergency, legacy) -> Capabilities` on every read:

| Facet | `enabled` | `activating` | `registration_required` | `not_available` |
|---|---|---|---|---|
| `outbound_sms` | `sms_status = ready`, and for a US sender `a2p_status = ready` (or toll-free `Verified`) | Configuration or a submitted registration is in progress | The tenant has no registration for this sender; `next_step` names `register_us_a2p` or `verify_toll_free` | The row has no `sms`, `sms_use` forbids sending, or the registration cannot be had for this tenant (`reason = "brand_country_not_supported"`), or the number is `legacy_unpapered` |
| `inbound_sms` | `sms_status = ready` | `sms_status` pending or configuring | – | The row has no `sms` |
| `inbound_calls` | `voice_status = ready` and a route other than `none` | Route provisioning in progress | No route set yet; `next_step` names `set_voice_route` | The row has no `voice` |
| `outbound_calls` | A `vapi` route ready, and for a US number a registered emergency address | As above | `next_step` names `enable_emergency` or `set_voice_route` | The row has no `voice`, the route kind cannot place calls, or a US number of a tenant with no US address (`reason = "no_us_emergency_address"`, [§14.8](#148-emergency-calling)) |

Each facet is `{ "state", "reason"?, "next_step"? }`, where `next_step` is `{ "who": "person" | "operator" |
"agent", "kind", "request_id"?, "console_url"? }`. The values are AgentPhone's for its own `outboundSms`
field (`enabled`, `activating`, `registration_required`; npm `agentphone` 1.0.25, `PhoneNumberResponse`, read
2026-10-26), so an agent that has met one meets the other. The send pipeline's A2P gate
([§12.2](#122-policy-pipeline) step 8) and the call routes read the same function, so a facet never says
`enabled` for something a route would refuse.

The number object also reports `setup_state` (`ready`, `carrier_pending`, `configuring_sms`,
`configuring_voice`, and `registration_pending` when only a registration stands between the number and
everything its row promises) and `provisioning { status, code, retryable }`, so an integrator's existing copy
keyed to the codes keeps working. `ready` means every capability of the row is `enabled`. The code set is
Pylota's, unchanged:

| `code` | When | Terminal |
|---|---|---|
| `no_numbers_available` | Search found no candidate, or the chosen number was taken (85001) | no: a new request can search again |
| `regulatory_rejected` | The provider rejected requirements, a group is not approved for the row, or a deadline passed | yes |
| `carrier_billing` | `402` or code 20100 from the provider: the operator's provider balance is short ([§18.4](#184-provider-side-backstops-and-balance)) | yes, until the operator acts |
| `carrier_error` | Any other provider refusal | no |
| `carrier_order_uncertain` | An attempt is `uncertain` for more than 24 hours ([§6.3](#63-spend-intent-customer_reference-and-uncertain)) | yes, until resolved |
| `voice_configuration`, `sms_configuration` | A facet could not be configured or read back after its retries | no |
| `provision_in_progress` | Only "another run holds the lease" (never a long review) | no |
| `operator_deleting` | The tenant is erasing or the identity deleting | yes |
| `not_configured` | `PM_TELNYX_API_KEY` missing, the tenant's provider resources missing and not creatable, or a `401`/`403` from the provider | yes, until the operator acts |

New information goes into new fields, never new codes: `requirements_status`, `deadline_at`,
`action_needed { kind, message, deadline_at }` and `review_started_at`. Provider IDs are never returned to
tenant or identity keys (Pylota parity 3); platform and partner keys get them in `provider`.

### 6.5 Requirements, deadlines and long reviews

- **`requirements_pending`** emits `phone_number.action_needed` with the provider's comments (untrusted
  text, at most 2,000 characters), notifies the tenant's owners and the identity's accountable human, and
  shows in the console's "Needs a person" with the deadline. The fix is a new `submit_compliance` request
  (approved by a person) whose execution also updates the order's requirement group
  (`PATCH /sub_number_orders/{id}` or `POST …/requirement_group`).
- **Deadlines** produce reminders 72 and 24 hours before `deadline_at`. A deadline that passes ends the
  order at the provider; the reconciler then sees it `cancelled` or `deleted`, confirms with a complete
  number listing that no live number of the order remains, sets the number `failed`
  (`regulatory_rejected`, `details.reason = "deadline_passed"`), clears the spend intent and fails the
  request. A cancelled or deleted order never strands the tenant: the next request orders
  again.
- **Long reviews** back off: the reconciler checks `under_review` numbers at 1 hour, 6 hours, then daily, and
  raises `phone_review_long` at 7 days; nothing loops, and "awaiting review" never depends
  on a sweep, because the provider's own state drives the transition.
- **Activation before documents.** Telnyx allows an international number to be bought before its documents
  are verified, but "inbound calling will not be activated until documentation has been uploaded and
  verified" (Telnyx support articles, read 2026-10-24). Orders for rows with `requirement_group = required`
  are placed only with an approved group, except rows with `pre_approval: false`, whose complete group is
  reviewed through the order and whose number is not activated until it is approved ([§3.3](#33-rules-the-catalogue-enforces)); for other rows the voice facet stays
  `pending` until the provider says the number is fully active.

### 6.6 Who moves an order

Four movers, each idempotent and safe under the execution lease (Pylota parity 6):

1. **Provider events**: `number_order.complete` and the order notifications ([§9](#9-provider-webhooks)),
   resolved by `customer_reference`.
2. **The every-minute cron** (`crons/phone.rs`): requests and numbers whose `next_check_at` has passed,
   oldest first, at most 50 per run, each as a `Reconcile` message on `pm-phone`.
3. **Read-repair**: `GET` of a number or request whose `next_check_at` has passed queues a `Reconcile` (it
   never calls the provider itself) and writes `next_check_at = now + 60 s`, so a client polling every
   5 seconds causes one provider read a minute at most (Pylota parity: written backoff).
4. **The nightly reconciler** ([§17](#17-reconciliation)).

### 6.7 One-call purchase

For an agent on a tenant that has finished onboarding for the country, inside its standing budget, getting a
number is one call, with no search first and no person in the path:

```text
POST /v1/identities/{identity_id}/phone-numbers   { "country": "GB" }                      → 201, the number
POST /v1/identities/{identity_id}/phone-numbers   { "country": "US", "area_code": "512" }  → 201, the number
```

The design follows AgentPhone's `POST /v1/numbers`, whose fields are all optional and which buys at once from
a prepaid balance (npm `agentphone` 1.0.25, `CreateNumberRequest` and `reference.md`, read 2026-10-26), with
three differences: the country is required (tenants here are UK-first, so a silent US default would buy the
wrong number); the approval is enforced on the server by the budget or a person, never by a flag the caller
sets (AgentPhone's MCP tool `buy_number` takes `confirm: true` from the agent itself, `agentphone-mcp` `src/tools.ts`
at `a78c5df`, read 2026-10-26); and the number is registered in the tenant's own name.

`POST /v1/identities/{identity_id}/phone-numbers` (`phone:request`, every key level; an identity key only for
its own identity; `Idempotency-Key` **required**):

| Field | Rule |
|---|---|
| `country` | Required, ISO 3166-1 alpha-2 |
| `type` | Default: the country's row with `default_for_country` (GB `mobile`, US `local`) |
| `area_code`, `contains`, `locality` | Optional search criteria. A row without area codes (GB mobile) refuses them (`400 invalid_request`, `details.reason = "location_not_supported"`), as AgentPhone's search does for UK numbers, rather than ignoring them |
| `number`, `quote_token` | Optional: an exact number from a search ([§6.1](#61-search)); otherwise `selection.mode = "any"` |
| `expires_at` | Optional ([§6.8](#68-throwaway-numbers)) |
| `primary_for` | Optional `["sms"]`, `["voice"]` or both |
| `acknowledge_not_available` | Optional `true`: buy even though a `follow_on` entry says a capability cannot be had for this tenant (for example texting from a US number for a GB-registered tenant) |
| `client_reference` | Optional, 1–128 printable ASCII, echoed in events |

The handler creates the `purchase_number` request exactly as `POST …/phone-requests` would, with the same
checks, quote, digest, budget decision ([§5.7](#57-the-standing-purchase-budget)) and onboarding block
([§5.8](#58-requests-that-wait-on-onboarding)). Then:

- **Covered and not blocked**: it claims the request and runs the execution inline, up to the order call and
  its answer, within 15 seconds (`buy_inline_budget_ms`), under the same lease, spend intent and
  `customer_reference` as the queued executor ([§6.3](#63-spend-intent-customer_reference-and-uncertain)).
  When the provider accepts the order it answers **`201`** with the number object (status `under_review`,
  `configuring` or `active`, its capability facets, and the request ID). The rest (configuration,
  reviews) continues in the queue and is reported by `phone_number.status_changed`. If the 15 seconds run out
  first, the run continues in the queue and the answer is `202`.
- **Pending approval or blocked**: **`202`** with the request (`status`, `blocked_by`, `details.outside_budget`,
  the quote with `follow_on`, and `next_step` naming who must act).
- **Refused**: the request's errors ([§22](#22-failure-handling)).

**A taken number is not the agent's problem.** With `selection.mode = "any"`, a provider answer that the
number was taken (85001) is a definitive refusal proved by the reference listing; the run then searches again
and orders the next candidate within the same request, up to 5 candidates (these attempts are separate from
the 3 allowed for uncertain outcomes), and only then fails with `no_numbers_available`. With an exact
`number`, the answer is `409 phone_number_taken` with `details.alternatives[]`: up to 5 fresh candidates in the
search's shape, each with its quote and `quote_token`, so the agent picks one and calls again with a new
`Idempotency-Key`, as AgentPhone's purchase answers `409` "with fresh alternatives in this same shape"
(`reference.md`, Numbers, read 2026-10-26).

The request routes stay for integrators that want a person to review every purchase, and for the console.

### 6.8 Throwaway numbers

An agent sometimes needs a number for one task (a sign-up whose code arrives by text) and none afterwards.
A forgotten number keeps billing, so a purchase may carry `expires_at` (from 1 hour to 365 days after the
request), as AgentPhone's purchase does ("`expires_at` auto-releases", PyPI `agentphone` 0.20.1,
`async_client.py`, read 2026-10-26):

- The approval of the purchase (a person's, or the budget's) covers the release at `expires_at`: the digest
  includes it, and the request stores `release_at`.
- 24 hours before (or at purchase, when the life is shorter), `phone_number.expiring` is emitted and the
  tenant's owners see it in "Needs a person" when the number has SMS from the last 24 hours.
- At `release_at` the every-minute cron creates a `release_number` request already `approved`
  (`approved_by = "purchase"`, linked to the purchase) and queues it. The release follows every rule of
  [§7.4](#74-release) (ownership stamp, pending orders, route removal) and the 90-day quarantine of
  [§7.5](#75-quarantine-of-released-numbers). A release that cannot run (a pending port-out) waits and
  retries; it never skips a safety rule.
- `PATCH …/phone-numbers/{number_id}` with an earlier `expires_at` is allowed to `phone:request` (an agent
  may give a number up sooner); a later one, or `null` (keep it), needs `phone:manage`, because it extends
  spending.
- Months are charged whole ([§18.2](#182-what-is-charged)): a number kept for one hour costs its setup
  price and one month. The quote says so.

## 7. Numbers on identities

### 7.1 The directory and history

`phone_numbers` is the directory: an inbound SMS or call is routed with one lookup on the live E.164
(`UNIQUE INDEX … WHERE status IN ('ordering','uncertain','under_review','requirements_pending','configuring','active','suspended','port_out_pending','porting_in','releasing')`).
Lookups that route traffic (inbound SMS, calls, the integrator's lookup) match only `configuring`,
`active`, `suspended` and `port_out_pending` rows (Pylota parity 2: active numbers only). A row that is
`released`, `ported_out` or `failed` is never deleted while its identity exists: it records
`active_from` and `active_until`, and the identity's SMS and calls keep their `number_id`, so history stays
readable after the number has gone and is scoped to the ownership period.

`GET /v1/phone-numbers/lookup?number=+447700900123` (`phone:read`, tenant, partner or platform key)
returns the identity and number IDs of a live number in the key's scope, or `404 phone_number_not_found`,
the same answer for a number of another tenant (NFR-SEC-1).

### 7.2 Roles, moves and limits

- **Role.** Each number has `role` `primary` or `alias` per capability: the identity's primary SMS number
  is the default `from` of a send; its primary voice number is the caller ID of calls it places. Promoting a
  number (`PATCH …/phone-numbers/{number_id}` with `primary_for: ["sms"]`, `phone:manage`) demotes the
  previous one; existing SMS conversations keep replying from the number the counterparty wrote to, as mail
  threads do ([Threading](threading.md)).
- **Move.** `POST …/phone-numbers/{number_id}/move` with `to_identity_id` (`phone:manage`, within one
  tenant) moves a live number to another identity. No provider call is made; the voice route is kept; the
  history stays with the old identity, and the new identity's history starts at the move. Audit
  `phone_number.move`, event `phone_number.status_changed` with `reason: "moved"`.
- **Limits.** At most `phone.max_numbers_per_identity` live numbers per identity (default 5, lower-only,
  maximum 20) and `phone.max_numbers_per_tenant` per tenant (default 20, lower-only, maximum 200), counting
  rows in any live status and requests in `approved`, `executing` and `in_progress`; a request over either
  gets `422 phone_number_limit_reached` at creation and again at execution.

### 7.3 Identity and tenant states

| State | Inbound SMS and calls | Sends and calls placed | Requests |
|---|---|---|---|
| Identity paused | Stored and evented | Refused, `409 identity_paused` | Created; executed (a paused identity's numbers still cost money and must stay manageable) |
| Identity without an accountable human | Stored | Refused, `409 identity_owner_required` (FR-IDN-2) | Created; executed |
| Tenant suspended | Stored | Refused, `403 tenant_suspended` | Refused, `403 tenant_suspended`; a claimed run stops at its next guard |
| Identity deleting | Refused: `DELETE /v1/identities/{id}` answers `409 identity_has_phone_numbers` while the identity holds a live number, so a deletion never strands a number that keeps billing ([P35](#34-edge-cases)) | – | – |
| Tenant erasing | Dropped after the `phone_release` step ([§24](#24-privacy)) | Refused | Canceled (`tenant_erasing`) |

### 7.4 Release

Releasing is a `release_number` request approved by a person. The run, under the execution lease (Pylota
parity 11, with its safety rules kept):

1. **Discover.** Read the number's stored provider IDs; if missing, find the order by `customer_reference`,
   then the number by exact E.164 in a complete listing. A truncated listing is never read as absence.
2. **Wait on pending orders.** A number whose order is still `pending` is not released; the run ends and
   is retried in an hour.
3. **Prove ownership.** The provider resource must carry this number's ownership stamp
   (`customer_reference = "pi:n:" + number_id` and the tag `pi:t:{tenant_id}`, written at configuration,
   [§8](#8-tenant-provider-resources)); a live number that lacks it is refused and raises
   `phone_release_unattributable`, because on a shared account it may belong to another tenant. Adopted numbers get their stamp at adoption ([§7.6](#76-adoption-for-pylotas-migration)).
4. **Remove the voice route** ([§14](#14-voice)): delete the Vapi phone number (a `404` is success), clear
   forwarding, unassign the connection.
5. **Delete** with `DELETE /phone_numbers/{id}`; a `404` is success. A terminal order
   (`cancelled`, `deleted`) completes only when the number listing is empty and complete.
6. **Finish.** Number `released` with `active_until`, port-out PIN deleted from the vault, monthly charges
   stop at the end of the current month (no refund of a month started, matching the provider), event
   `phone_number.status_changed` (`released`), request `completed`.

### 7.5 Quarantine of released numbers

A released number returns to the provider's inventory after a hold whose length Telnyx does not document
(unknown 7.3.8 of the study). Codes and messages meant for the previous holder can arrive at the next. So
Pylota Identity refuses to sell a number released by one of its tenants to a **different** tenant for 90
days (`phone_number_releases`, keyed by `HMAC(PM_HASH_KEY, e164)`, with the tenant and `released_at`):
search drops such candidates, and the executor refuses them with `no_numbers_available` if they appear
anyway ([P39](#34-edge-cases)). The same tenant may buy it back at once. The hash row is deleted after 90 days.

### 7.6 Adoption, for Pylota's migration

Pylota's numbers are in a TREFT-owned Telnyx account today. If Pylota Identity Cloud uses that same account
as its provider account (same legal owner), Pylota's numbers move without porting:
`POST /v1/platform/phone-numbers/adopt` (`platform:ops`) with `identity_id`, the E.164 and, optionally, the
existing Vapi phone number ID. The handler finds the number in a complete listing, refuses one already in
the directory or carrying another tenant's stamp, writes the ownership stamp and the tenant tag, moves the
number to the tenant's messaging profile and connection (reading each back), records the Vapi number for
the `vapi` route ([§14.2](#142-vapi)), and creates the row `active` with `adopted_at`. An adopted number
whose order used Pylota's platform-wide requirement group gets `compliance_status = "legacy_group"`, and its
tenant must have the number re-papered under its own approved profile by its `legacy_deadline`
(`phone_compliance_legacy` notifications at 60, 30 and 7 days), because no number may stay registered in
TREFT's name ([§3.3](#33-rules-the-catalogue-enforces)). A tenant that cannot have an eligible profile (a
sole trader, refused under [§1.1](#11-sole-traders-refused-owner-decision-2026-10-26)) keeps its number working and gets the dated exit path of §1.1; no adopted number is ever released by a
deadline. Adoption pre-fills the tenant's onboarding ([§4.7](#47-phone-onboarding)) from the platform group's
non-sensitive fields, so re-papering is a confirmation and a document upload, not a new form. Adoption is audited
(`phone_number.adopt`) and emits `phone_number.status_changed` with `reason: "adopted"`. If the owner
chooses a different Telnyx account, numbers move by port-in instead ([§15](#15-porting-in)).

## 8. Tenant provider resources

Before the first billable action for a tenant, the executor **ensures** the tenant's own resources at the
provider, recorded in `phone_tenants` (one row per tenant):

| Resource | Settings | Why per tenant |
|---|---|---|
| Messaging profile `pi-{tenant_id}` | `webhook_url = https://{PM_API_HOST}/hooks/telnyx`; `whitelisted_destinations` = the tenant's `phone.sms.allowed_destination_countries`; `daily_spend_limit` (USD) as a backstop ([§18.4](#184-provider-side-backstops-and-balance)); content redaction on; STOP, START and HELP auto-responses per country with the tenant's brand name (`phone.sms.brand_name`, default the profile's trading name) | Opt-out block rules apply to the whole profile; one profile per tenant means a renter's STOP to one operator never blocks another; sender reputation is per tenant |
| Credential connection `pi-{tenant_id}` | Random SIP user name and password, **sealed** (`sip_sealed`); `webhook_event_url` = the hooks URL; `call_cost_in_webhooks: true`; outbound through the tenant's outbound voice profile | Voice routing and call events per tenant |
| Outbound voice profile `pi-{tenant_id}` | `whitelisted_destinations` = `phone.calls.allowed_destination_countries`; `daily_spend_limit`; `concurrent_call_limit` = `phone.calls.concurrent_limit` (default 2); `max_destination_rate`; recording off | A runaway outbound route is bounded per tenant at the provider |
| Billing group `pi-{tenant_id}` | – | Usage reports attribute cost per tenant |

Each create is preceded by a lookup by name and tag in a complete listing, because none of these `POST`s
takes an idempotency key: a run that crashed after a create adopts the resource instead of creating a
second one ([P41](#34-edge-cases)). Each resource is read back after creation and after every change. A
policy change that affects a resource (allowed countries, caps) sets `phone_tenants.sync_due_at`, and the
every-minute cron applies it with `PATCH` and a read-back. Content redaction and the auto-response
settings are re-checked by the nightly reconciler; the exact effect of each redaction level is verified by
`live::phone::redaction_levels` before launch (Telnyx documents two levels without saying what each keeps).

STOP replies are sent by the provider from these auto-responses, and **Pylota Identity never sends a
confirmation of its own**, so a recipient never gets two. On US
toll-free numbers the carrier's own opt-out ("NETWORK MSG") also replies, which neither Telnyx nor Pylota
Identity can suppress (Telnyx opt-out guide, read 2026-10-24); the integration guide says so.

**Ownership stamp.** When a number becomes carrier-active (or is adopted), the configure step writes
`customer_reference = "pi:n:{number_id}"` and the tags `pi`, `pi:t:{tenant_id}` and `pi:n:{number_id}` on
the provider's number resource (`PATCH /phone_numbers/{id}`), sets a random `external_pin` whose value goes
to the vault ([§16](#16-the-port-out-guard)), and reads the resource back. Release and the reconciler trust
only this stamp. Telnyx's research table lists `deletion_lock_enabled` on numbers, but the
`PATCH` schema at `72cf10d738ff` does not take it; if a contract test finds a supported way to set it, the
configure step sets it and release clears it first, otherwise the approval rule is the only lock.

## 9. Provider webhooks

### 9.1 The endpoint

`POST /hooks/telnyx` on the API host, unauthenticated by key, like the SES hooks: every messaging profile,
credential connection, notification channel, 10DLC brand and campaign points its webhook at it. It is in
the router's table of unauthenticated routes ([Security § 4.7](security.md#47-unauthenticated-routes)), has
its own rate-limit bucket keyed by client network (the API bucket, 600 a minute), and accepts at most
256 KB of body (Telnyx event bodies are small; a larger one is refused with `413`).

### 9.2 Verification

In every environment, test tenants and staging included:

1. `telnyx-signature-ed25519` (base64, 64 bytes) and `telnyx-timestamp` (decimal Unix seconds) must be
   present, or `403 invalid_signature`.
2. `|now − telnyx-timestamp| ≤ 300 s` (Telnyx states no window; 5 minutes is Pylota's and Standard
   Webhooks' tolerance), or `403 invalid_signature` with `details.reason = "timestamp"`.
3. The signature must verify with Ed25519 over the bytes `telnyx-timestamp | "|" | raw body` against
   `PM_TELNYX_PUBLIC_KEY` (32 raw bytes, base64), with `ed25519-dalek =3.0.0` already pinned for agent keys
   ([Security § 7.1](security.md#71-choices)); the raw body is verified before it is parsed. A missing key
   rejects everything (`503 unavailable`, `details.reason = "not_configured"`).
4. The source address (`CF-Connecting-IP`) is logged as a pseudonym and compared with Telnyx's published
   messaging subnet `192.76.120.192/27` for metrics only, never as a gate, because voice and notification
   webhooks are not documented to come from it.

### 9.3 Acknowledge, stage, queue

Telnyx needs a `2xx` within 2 seconds and makes 6 attempts in all. The handler does no business work:

1. parse the envelope only as far as `data.id`, `data.event_type` and `data.occurred_at`;
2. write the raw body to R2 at `phone-staging/{yyyy}/{mm}/{dd}/{event_id}.json` (the key is the provider
   event ID, so a duplicate overwrites the same bytes);
3. send `ProviderEvent { v: 1, kind: "provider_event", event_id, event_type, occurred_at, key }` to
   `pm-phone` (pointer only, no content);
4. answer `200` with an empty body.

A failed R2 write or queue send answers `503`, and Telnyx retries. The handler's p99 budget is 500 ms
(`phone_webhook_ack_ms`). Nothing in the handler reads D1, so a D1 outage never costs an event.

### 9.4 Resolving the tenant

The consumer resolves the owner from facts the provider controls, never from a field a sender could set,
and cross-checks a second fact where there is one ([P46](#34-edge-cases)):

| Event | Owner from | Cross-check |
|---|---|---|
| `message.received` | `payload.to[0].phone_number` in the routing directory | `payload.messaging_profile_id` equals the tenant's profile |
| `message.sent`, `message.finalized` | `payload.id` in `phone_message_index` (written at dispatch) | `from` is a number of the same tenant, or the sender was substituted (recorded, [§12.4](#124-delivery-receipts-and-the-sender-used)) |
| `number_order.*`, order notifications | `customer_reference` (`pi:{request_id}:{attempt}`) | The order's numbers equal the request's |
| `porting_order.*` | The porting order ID in `phone_requests.result` | `customer_reference` |
| `portout.status_changed` | The phone number in the directory | The provider number ID stored on the row |
| `requirement_group.*` | The group ID in `phone_requirement_groups` | The group's tag |
| `10dlc.brand.update`, `10dlc.campaign.update` | The brand or campaign ID in `phone_registrations` | – |
| `call.*` | `payload.to` (inbound) or `payload.from` (outbound) in the directory | `payload.connection_id` equals the tenant's connection |

A failed cross-check discards the event's effect, keeps it as `ignored_mismatch` in
`phone_provider_events`, and raises `phone_event_mismatch` for the operator: on a shared account a
misrouted event is a tenant-isolation incident, never data for whichever tenant matched first.

### 9.5 The `pm-phone` consumer

One queue, `pm-phone`, with its dead-letter queue `pm-phone-dlq`, carries five kinds of message, each
absorbed idempotently ([Design § 7](index.md#7-idempotent-queue-consumers)):

| Kind | Identity | How a repeat is absorbed |
|---|---|---|
| `provider_event` | `event_id` | `INSERT OR IGNORE INTO phone_provider_events`; a row already `processed` acks and deletes the staging object |
| `execute` | `request_id` | The claim of [§5.5](#55-execution-leases-and-attempts) admits one run at a time; a request no longer runnable acks |
| `reconcile` | target ID + `next_check_at` | The handler re-reads the target and acts only on a change |
| `send_sms` | `sms_id` | The mailbox's transport claim admits one provider call per message ([§12.3](#123-dispatch-provider-idempotency-and-uncertain)) |
| `configure` | number ID + facet | Each facet step is a read-then-write with read-back |

Provider events are applied in `occurred_at` order per resource: the consumer ignores an event older than
the last one applied to the same resource (`last_event_at` on the row), so a late `message.sent` never
overwrites `delivered` ([P47](#34-edge-cases)). After the effect commits, the staging object is deleted;
an R2 lifecycle rule removes any left after 15 days, matching the inbound staging rule
([Data model § 4](data-model.md#4-r2-objects)).

## 10. Inbound SMS

### 10.1 Steps

For `message.received` the consumer:

1. **Routes.** Resolve the number and cross-check the profile ([§9.4](#94-resolving-the-tenant)). An
   unknown number, or one `released`, `ported_out` or `failed`, is recorded `ignored_unrouted` and counted;
   nothing is stored ([P48](#34-edge-cases)).
2. **Parses.** `from` (E.164, a short code or an alphanumeric sender), `to`, `text`, `parts`, `encoding`,
   media, `received_at`, the provider message ID, and `autoresponse_type` (`STOP`, `HELP`, `START`) when
   the provider classified a keyword. Text is normalised to NFC and capped at 10,000 characters
   (`truncated` flag above).
3. **Applies consent keywords** ([§10.5](#105-opt-out-keywords-and-the-consent-ledger)).
4. **Classifies** the steering scan, the verification match ([§11.1](#111-extraction)) and the
   quarantine decision ([§10.2](#102-quarantine-rules-for-sms)), and loads the facts the mailbox needs (the
   tenant's policy, the identity's receive lists, approved service-ledger entries when a code matched),
   following the inbound consumer's pattern of passing D1 facts into the mailbox
   ([Inbound › Steps](inbound.md#steps), step 12).
5. **Stores** with `MailboxRequest::IngestSms`: one transaction that finds or creates the conversation
   (number, counterparty), inserts the `sms_messages` row (deduplicated on the provider message ID), the
   media rows, the `sms_verifications` row when one applies, the screening state, and the outbox event.
6. **Post-commit.** Media objects are fetched and stored ([§10.4](#104-mms-media)); a `screening` SMS is
   queued on `pm-screen` ([§10.3](#103-screening)).

Inbound SMS is never refused for a plan or spend reason (as FR-BILL-8 for mail); it is counted, priced
where the provider charges for it, and settled after the fact ([§18.2](#182-what-is-charged)).

### 10.2 Quarantine rules for SMS

SMS has no authentication verdict: the sender is what the carrier reports, and sender IDs can be forged.
Every inbound SMS carries the trust flag `sender_unverified`. The decision, first match wins, is
`core::phone::sms::quarantine`:

| Order | Condition | `status` | `quarantine_reason` |
|---|---|---|---|
| 1 | The sender is on the identity's or tenant's receive-block list (phone entries of the existing lists, [Parity additions § 3](parity-additions.md#3-identity-scope-allow-and-block-lists-fr-out-17)) | `hidden` | `blocked_sender` |
| 2 | The sender exceeded `phone.inbound.per_sender_per_hour` (default 30) for the identity, or the identity exceeded `phone.inbound.per_identity_per_hour` (default 600) | `throttled` | – |
| 2b | A verification match on a number named by an active vault login item's phone channel, from one of its senders; or any verification match while `vault.codes_into_fills_only` is true and the sender is not receive-allowed ([§11.4](#114-codes-for-the-vault)) | `hidden` | `vault_code` |
| 3 | A verification match, `accounts.require_approval` true, and no approved service-ledger entry matches ([§11.2](#112-the-gate)) | `quarantined` | `account_unapproved` |
| 4 | A verification match that is unsolicited, and `quarantine.unsolicited_otp` true | `quarantined` | `otp_unsolicited` |
| 5 | Screening (`screening.mode = hold`): the scan is decisive, or the SMS is held for a verdict ([§10.3](#103-screening)) | `quarantined` or `screening` | `steering_suspected` |
| 6 | Otherwise | `received` | – |

`hidden` and `throttled` SMS are stored for audit, never evented and never answered; a `vault_code` SMS is
also kept from API keys' reads ([§11.4](#114-codes-for-the-vault)). Quarantined and
screening SMS are visible only to keys holding `quarantine:review`, and released through
`POST …/sms/{sms_id}/release` under the quarantine release rule (FR-CON-6), exactly as mail is. The STOP,
START and HELP keyword messages themselves are never quarantined: their consent effect applies whatever
rule 1 to 5 decides ([P50](#34-edge-cases)).

### 10.3 Screening

An SMS is screened as mail is ([Screening hold](screening.md)): the deterministic scan
`core::injection::scan` runs on the text at consumer step 4 and sets `prompt_injection_suspected` and the
severity; in `hold` mode a decisive scan quarantines (rule 5), and an eligible SMS that the scan does not
decide is stored `screening` and queued as `ScreenJob::Sms { identity_id, sms_id }` on `pm-screen`, whose
classifier, deadline alarm, fail-closed default (`screening.on_unavailable`) and verdict merge are shared
with mail. The model input frames the text as an SMS from an unverified sender. Feedback and the summary
count SMS separately (`channel: "sms"`). An SMS of fewer than 20 characters, a keyword message, and a
message from the identity's own numbers are not eligible and skip the model ([P51](#34-edge-cases)).

### 10.4 MMS media

US long codes can receive MMS (Telnyx: MMS from US long codes and short codes). Media URLs in the payload
point at the provider's storage; the consumer fetches each, at most 10 items and 5 MB each (the provider's
own MMS limits are re-checked at build time), only from hosts in a compiled list of Telnyx media hosts
discovered by `live::phone::mms_media_hosts` (no request field names a host:
[Security § 9](security.md#9-ssrf-controls)), stores them at `t/{ten}/i/{idn}/sms/{sms}/m/{n}`, classifies
them with `core::attach` (a risky item quarantines the SMS, `risky_attachment`), and serves them only through
`GET …/sms/{sms_id}/media/{n}` (`phone:read`). A media fetch that fails is retried for one hour, then the
item is recorded `unavailable` (Pylota's attachment gate, parity 8).

### 10.5 Opt-out keywords and the consent ledger

The **consent ledger** (`phone_consent`) records, per tenant and messaging profile (the tenant's brand),
per counterparty, whether texting is allowed. The counterparty is stored as `HMAC(PM_HASH_KEY, e164)` with a
masked hint (`+44 •••• ••0123`), never in clear, so an erased counterparty's opt-out can still be honoured
([P53](#34-edge-cases), the rule of [I7](../edge-cases.md) for suppressions).

| Entry | Set by | Effect on sends |
|---|---|---|
| `opted_out` | An inbound STOP keyword (the provider's `autoresponse_type: STOP`, or Pylota Identity's own match of the keyword list in [§3.1](#31-rows-and-fields) of the catalogue for the sender's country), `POST …/phone-consent` with `state: "opted_out"` (`phone:send`, any key: an agent that reads "please stop texting me" records it), or the console | Every send to the counterparty from the tenant's profile is refused (`403 recipient_opted_out`) |
| `opted_in` | An inbound START or UNSTOP keyword, or `POST …/phone-consent` with `state: "opted_in"` and `evidence` (`phone:manage`: an agent cannot fabricate consent) | Clears `opted_out`; for `kind: "marketing"` sends, an `opted_in` entry with `purpose: "marketing"` and evidence is required (`403 consent_required`) |
| No entry | – | Transactional sends allowed; marketing refused |

Each change writes the entry with `source` (`keyword`, `api`, `console`, `provider_sync`), `evidence`
(1–1,000 characters of untrusted text, for example the form wording and time a renter ticked), the SMS ID
when a keyword caused it, and emits `phone_consent.changed` (with the masked hint only). HELP is answered
by the provider's auto-response and changes nothing. The provider's own block list is the backstop: a send
it refuses with 40300 is recorded as `opted_out` with `source: "provider_sync"`, and the nightly reconciler
compares `GET /messaging_optouts` with the ledger ([§17](#17-reconciliation)). Because each tenant has its own
profile, a STOP never crosses tenants at either layer.

## 11. Verification codes over SMS

### 11.1 Extraction

`core::phone::verify::find_code(text) -> Option<Code>` applies the same trigger keywords and code patterns
as mail's `core::classify::find_verification` ([Inbound › Verification codes](inbound.md#verification-codes-and-unsolicited-otp-e5))
to the SMS text, without the link rule (a link in an SMS is never a verification object: it is untrusted
text). A match is **solicited** when an `sms/wait` for the receiving number was registered (with a matching
`from` or `account_id`) at most 30 minutes before the SMS arrived; otherwise it is unsolicited (the E5 rule).

### 11.2 The gate

Where the tenant's `accounts.require_approval` is true, a code reaches an agent only through an approved
service-ledger entry, as for mail ([Service sign-up ledger § 5](service-accounts.md#5-the-verification-gate)).
SMS senders cannot be authenticated, so the match is stricter: an entry matches an SMS when

1. the entry belongs to the receiving identity, is `approved`, and names the receiving number in its new
   field `phone_number_id`, or names no number and the identity has only one live SMS number;
2. the SMS arrived while a `sms/wait` naming that entry (`account_id`) was registered, or within 30 minutes
   of one; and
3. when the entry lists `sms_senders` (up to 10 E.164 numbers, short codes or alphanumeric sender IDs),
   the sender is one of them. An entry with no `sms_senders` matches any sender, but only under rule 2.

Codes that match no entry are quarantined `account_unapproved` (rule 3 of [§10.2](#102-quarantine-rules-for-sms)),
never dropped. The service ledger gains the optional fields `phone_number_id` and `sms_senders` on
`POST …/accounts`; existing entries are unchanged.

### 11.3 `GET /v1/identities/{identity_id}/sms/wait`

`phone:read`, any key level; it mirrors mail's `wait` ([Inbound › The wait handler](inbound.md#the-wait-handler-e4)):

| Parameter | Rule |
|---|---|
| `number_id` | The receiving number; default the identity's primary SMS number |
| `from` | A sender (E.164, short code or alphanumeric); with `kind=verification` either `from` or `account_id` is required |
| `account_id` | An approved service-ledger entry of the identity (required with `kind=verification` when `accounts.require_approval` is true; otherwise `403 policy_denied`, `details.reason = "account_not_approved"`) |
| `kind` | `any` or `verification` |
| `timeout` | 1–60 seconds, default 30 |
| `since` | RFC 3339; default the request's start |

The handler registers the wait in the mailbox (`MailboxRequest::RegisterSmsWait { number_id, from,
account_id, ttl_ms }`, refreshed every 10 seconds), polls once a second, and answers
`200 { "sms": Sms | null, "verification": {…} | null, "timed_out": bool }`. The `verification` object is
filled only when the SMS is `received` (never `quarantined`, `screening`, `hidden` or `throttled`), the
match is solicited, the gate passed, the code is not claimed by a vault item ([§11.4](#114-codes-for-the-vault)),
and the `sms_verifications` row has not expired (24 hours). The first release sets `consumed_at`; a repeat
within one hour releases the same value (a client retrying a lost response); then the row is deleted. With
the vault's strict mode (`vault.codes_into_fills_only: true`), `sms/wait` with `kind=verification` answers
`403 policy_denied` with `details.reason = "codes_into_fills_only"`, as mail's `wait` does
([Vault § 9.2](vault.md#92-email-codes)).

### 11.4 Codes for the vault

The vault-bound variant hands a code to an authorised consumer without returning it to the model. It follows
the vault's model for one-time codes ([Vault § 9.3](vault.md#93-sms-codes)): a login item in the vault names
the number as its code channel (`otp_channel = { kind: "phone", number_id, senders }`), and the vault pulls the
code into an open `sms_otp` phase of a fill ticket, which the consumer (Pylota's agentic browser at the
service's exact origin) types into the form. The phone side provides three things:

1. **Withholding (quarantine rule 2b of [§10.2](#102-quarantine-rules-for-sms)).** An inbound SMS to a number
   named by an `active` login item's phone channel, whose sender is one of the channel's `senders` and whose
   text has a verification match, is stored `hidden` with `quarantine_reason = 'vault_code'`. It is invisible to
   every agent-facing surface (lists, conversations, `sms/wait`, the event feed, webhooks, MCP; no
   `sms.received` and no `sms.verification_received`), its text is withheld from API keys whatever their
   permissions (`content_withheld: "vault_code"`), a signed-in person sees it in the console, it is never
   released, and it and its `sms_verifications` row (recording `vault_item_id`) are deleted 24 hours after
   arrival. With `vault.codes_into_fills_only: true`, every SMS with a verification match on the tenant's
   numbers is withheld this way, except senders on the tenant's receive-allow list. The consumer loads the
   identity's active phone channels with its other facts (one D1 read, only when the SMS has a verification
   match), so the mailbox rule reads nothing more.
2. **The code source.** `phone::codes::SmsCodes` implements the vault's `SmsCodeSource`:
   `take(tenant_id, number_id, senders, since_ms, use_id) -> Option<OneTimeCode>` sends
   `MailboxRequest::VaultTakeSmsCode { number_id, senders, since_ms, use_id }` to the mailbox of the
   number's identity, which in one transaction selects the oldest unexpired, unconsumed `sms_verifications`
   row of that number with a matching sender and `received_at >= since_ms`, sets `vault_consumed_at` and
   `vault_use_id`, and returns its kind and value. A code is spent once, even if the vault's redemption fails
   afterwards (fail closed). The vault calls it only for a use created at most 10 minutes earlier and never
   for an unsolicited code (the vault's own rules).
3. **Erasure.** Withheld SMS and their code rows are covered by every erasure scope of [§24](#24-privacy).

No code value ever appears in a response, an event, a log or a model's context on this path. The interface
is the vault's ([Vault § 9.3](vault.md#93-sms-codes)): the channel lookup `vault::phone_channels` lives in the
vault crate, and the vault owns "rotate and show once" for managed secrets in partner custody
([Vault § 13.2](vault.md#132-managed-secrets); [P59](#34-edge-cases), [P60](#34-edge-cases)).

### 11.5 Redaction

With `phone.verification.redact: true` (the default; a guard field), every read and every event shows an
SMS whose text holds a verification match with the code replaced by `[code]`; the value lives only in
`sms_verifications` until it is released or expires. `sms.verification_received` carries the SMS ID, the
number ID, the masked sender, `account_id` and `kind`, never the value. Logs never hold SMS text or codes
([§23](#23-security-t2)).

## 12. Outbound SMS

### 12.1 The send route

`POST /v1/identities/{identity_id}/sms` (`phone:send`; `Idempotency-Key` **required**, except with
`?dry_run=true`):

```json
{ "from": "pnm_01JB…", "to": "+447700900123", "text": "Your car is ready at bay 4. Reply STOP to opt out.",
  "kind": "transactional", "conversation_id": null, "client_reference": "bk-2291-ready" }
```

| Field | Rule |
|---|---|
| `from` | A number ID or E.164 of **this** identity, `active` with `sms_status = ready`; default the identity's primary SMS number. Never a platform number and never another identity's (`404 phone_number_not_found`), so a tenant's traffic never leaves from a shared sender |
| `to` | E.164 (`400 phone_number_invalid`; `core::phone::e164` validates country code and length) |
| `text` | 1 to 10 segments (`core::phone::segments`: GSM-7 153 characters per segment when split, 160 for one; UCS-2 67 and 70), else `400 invalid_request` with `details.segments` |
| `kind` | `transactional` (default), `marketing` or `verification` (codes the integrator sends to its own users, as Pylota's sign-in fallback does) |
| `client_reference` | Optional, 1–128 printable ASCII, echoed in events |

The answer is `202` with the SMS object (`status: "queued"`), or the stored response on a replay with
`Idempotent-Replayed: true`. The parity contract Pylota calls today,
`send(from, to, text) → {sent, external_id, error_code}`, maps onto it in Pylota's adapter: `sent` is a
`202`, `external_id` is the `sms_` ID (which every later event and receipt carries), and the four codes come
from `401` (`api_key_missing`), `404 phone_number_not_found` or `409 phone_number_not_ready` on `from`
(`from_missing`), any other error envelope (`provider_error`) and a network failure (`network_error`).

### 12.2 Policy pipeline

`MailboxRequest::SubmitSms` runs the steps in order; the first refusal decides (`core::phone::sms::check`
is pure and takes the facts the handler loaded):

1. **Idempotency.** Same key and body: the stored response. Same key, other body: `409
   idempotency_conflict`. In progress: `409 request_in_progress`.
2. **Tenant and identity.** `403 tenant_suspended`, `409 identity_paused`, `409 identity_owner_required`.
3. **Ops switch.** `phone` at `paused`: `503 unavailable`, `details.reason = "phone_paused"`. At `on` and at
   `paused_purchases` the send continues: `paused_purchases` stops only orders, ports and registration fees
   ([§30](#30-rollout-telemetry-and-rollback)), and low provider balance sets it automatically, so texts keep working
   while the operator tops up ([P128](#34-edge-cases)).
4. **Test tenant.** A test tenant's SMS never reaches the provider: it is accepted and driven by the
   simulator (the last digit of `to` chooses `delivered`, `failed`, `delivery_unconfirmed` or `uncertain`),
   and the simulator's IDs are never written outside test tenants.
5. **Sender.** `from` as above, and the number has the `sms` capability with `sms_use = two_way` or
   `a2p_only`.
6. **Destination.** `to`'s country is in `phone.sms.allowed_destination_countries`, else `403
   policy_denied` (`details.reason = "destination_country"`); and the sending number's catalogue row marks
   that country `tested_ok`, else `403 policy_denied` (`details.reason = "destination_untested"` or
   `"destination_not_supported"`, [§3.5](#35-sending-across-borders)). Premium-rate and short-code
   destinations are refused (`403 destination_blocked`).
7. **Consent.** `opted_out` for this tenant's profile: `403 recipient_opted_out`. `kind: "marketing"`
   without a marketing `opted_in` entry: `403 consent_required`.
8. **US A2P.** When `from` is a US number: a long code needs `a2p_status = ready` for the number (its 10DLC
   campaign `MNO_PROVISIONED` and the number assigned); a toll-free number needs verification `Verified`.
   Otherwise `403 a2p_not_registered` with `details.registration` and `details.status`. This gate applies
   to every send, the agent tools included, and fails closed.
9. **Disclosure.** When `phone.disclosure.first_sms` is `required` and the conversation has no earlier
   outbound SMS, the tenant's disclosure text (`phone.disclosure.sms_text`, default
   "Automated assistant for {brand}.") is prepended; the segment count includes it.
10. **Quiet hours.** `kind: "marketing"` to a country whose catalogue row has quiet hours, inside them:
    `409 quiet_hours` with `details.next_allowed_at` (retryable, `Retry-After`).
11. **Caps.** `phone.sms.identity_daily_cap` (default 200), `phone.sms.tenant_daily_cap` (2,000) and, when
    set, `phone.sms.tenant_monthly_cap`, counted in `TenantQuota` per segment: `429 daily_cap_reached` with
    `details.cap` (`sms_identity`, `sms_tenant`, `sms_tenant_month`). All fail closed: an unreachable
    `TenantQuota` is `503 unavailable`, never a pass.
12. **Spend.** A `phone_spend` hold of the estimated price ([§18.3](#183-spend-caps)): `402
    spend_cap_reached` with `details.scope`.
13. **Accept.** One mailbox transaction writes the `sms_messages` row (`queued`), the conversation and the
    idempotency row; no event is emitted at acceptance, because the `202` carries the SMS. Then `SendSms`
    is queued on `pm-phone`.

A dry run runs steps 2 to 12 without holds and answers `200 { "would_send": true, "segments": n,
"price_pence": p }` or the refusal.

### 12.3 Dispatch, provider idempotency and `uncertain`

The `send_sms` consumer claims the message in the mailbox (`BeginSmsTransport`, one provider call per
message, as `BeginTransport` is for mail) and calls `POST /messages` with the tenant's messaging profile,
`from`, `to`, `text`, and the header `Idempotency-Key: pi-sms-{sms_id}`. Telnyx's OpenAPI document of
2026-10-24 declares that header on this route: same key and body within 24 hours replays a successful
response, a request still in progress answers `409`, and a different body answers `422`.

- **`2xx`**: store the provider message ID in the row and in `phone_message_index` (provider ID → tenant,
  identity, SMS), status `sent_to_provider`, settle nothing yet.
- **Definitive refusal**: status `failed` with `reason` mapped from the provider code (40300 opted out:
  also writes the ledger; 40309 destination not allowed; 40010 not 10DLC registered; 40329 toll-free not
  verified; 40333 profile spend limit; 40315 unhealthy sender; 40318 queue full is transient), hold
  released, `sms.failed`.
- **Unknown outcome**: the consumer repeats the **identical** request with the **same** idempotency key at
  30 seconds, 2 minutes and 10 minutes. Under Telnyx's contract a repeat cannot send twice: it either
  replays the first success or performs the one send. Only when all three repeats also end unknown does the
  SMS become `uncertain` (`sms.uncertain`), and it is never sent again automatically. These repeats are
  enabled only after `live::phone::send_idempotency` has shown, against the real API, that a repeated key
  returns the first message's ID and sends one SMS; until then the first unknown outcome makes the SMS
  `uncertain` at once ([P63](#34-edge-cases)).
- **Reconciling `uncertain`**: a later `message.sent` or `message.finalized` for this sender and recipient
  whose provider ID is unknown is matched to an `uncertain` (or `sent_to_provider` without ID) row of the
  same identity with the same `to` and the same text digest within 10 minutes: the row takes the provider
  ID and becomes `reconciled` then follows the receipt (`sms.reconciled`). The nightly reconciler does the
  same from message detail records. An integrator can also resolve it: `POST …/sms/{sms_id}/resolve` with
  `outcome: "sent"` or `"not_sent"` (`phone:send`), as mail's resolve works.

### 12.4 Delivery receipts and the sender used

| Provider event | SMS status | Notes |
|---|---|---|
| `message.sent` | `sent` | `sender_used` = the payload's `from`; if it differs from the requested sender, `sender_substituted: true` and `sms_sender_substituted_total` (alphanumeric fallbacks and destination-network replacements exist: Telnyx alphanumeric guide and France SMS guideline) |
| `message.finalized`, `to[0].status = delivered` | `delivered` | |
| `sending_failed`, `delivery_failed` | `failed` with the provider's reason | Spend settled at the provider's charge, which may be zero |
| `delivery_unconfirmed` | `delivery_unconfirmed` | Final; 10DLC long codes get carrier receipts only (Telnyx "Choosing a sender type") |
| Any receipt carrying `cost` | – | Settles the hold and writes the charge ([§18.2](#182-what-is-charged)) |

A receipt that arrives before the provider ID is stored is kept in `phone_provider_events` as `pending_target` for 15 minutes and re-applied when
`phone_message_index` gains the ID, instead of being acknowledged and lost ([P66](#34-edge-cases)).

### 12.5 Pacing

Telnyx queues messages above a sender's rate (0.1 a second per long code; 20 a second per toll-free
number) and discards them when `valid_until` passes (error 40014) (Telnyx messaging rate limits, read
2026-10-24). The mailbox paces dispatch per number from the catalogue row's `sender_rate`: a queued SMS
waits in `queued` until its number's next slot, so the provider queue never fills. US 10DLC campaign
throughput caps (AT&T per-minute classes, T-Mobile daily caps by brand score) are reported in
`phone_registrations` and applied as an extra per-campaign window when known.

## 13. US A2P: 10DLC and toll-free verification

US long-code A2P SMS needs a brand and a campaign registered with The Campaign Registry; each customer of an
ISV needs its own brand (Telnyx 10DLC ISV guide, read 2026-10-24). The design keeps Pylota's model (one
brand and campaign per tenant) and closes each Telnyx pitfall:

| Pitfall | Pylota Identity |
|---|---|
| (a) A campaign's `status` is `ACTIVE` from creation; vetting progress is in `campaignStatus` | Ready only when `campaignStatus = MNO_PROVISIONED`; `status` is ignored for readiness |
| (b) `identityStatus` has a fourth value, `VETTED_VERIFIED` | Brand approved when `identityStatus` is `VERIFIED` or `VETTED_VERIFIED`; an unknown value raises `phone_status_unknown` |
| (c) 10DLC events go only to the `webhookURL` set on the brand or campaign | Brand and campaign are created with `webhookURL = https://{PM_API_HOST}/hooks/telnyx`; event types `10dlc.brand.update` and `10dlc.campaign.update` are mapped |
| (d) Webhooks alone do not move a registration forward | The every-minute cron and the nightly reconciler run in the Worker |
| (e) The 10DLC paths sit under `/v2`, which the docs' overview omits | Every path is under the configured base (`…/v2/10dlc/…`), checked by `live::phone::a2p_paths` |
| (f) Brand creation takes no idempotency key and charges a non-refundable fee | Creation runs only inside a claimed `register_us_a2p` request with a spend intent; before creating a brand the run lists brands and adopts one carrying the tenant's tag or matching its EIN; campaigns use `referenceId = "pi:" + request_id`, which Telnyx keeps unique |
| (g) Sandbox or simulated IDs mistaken for real registrations | Test tenants use the simulator; a row's `provider_account_id` is always a live account, and a simulated ID is refused by a `CHECK` on `phone_registrations` (`provider_brand_id NOT LIKE 'sim-%'` outside test tenants) |
| (h) The brand's country, and assignment per number | The brand's country is the profile's registration country; number assignment is per number (`phone_numbers.a2p_status`), redone when a number joins or leaves the identity |
| (i) Toll-free numbers need toll-free verification, not 10DLC | No fallback types; toll-free has its own verification request |
| (j) Every send path must pass the registration gate, agent tools included | The gate is step 8 of the one send pipeline ([§12.2](#122-policy-pipeline)) |

Every fee-bearing step (brand registration, $4.50, **Conflict**: the brand endpoint says $4; external
vetting $41.50; campaign $30 up front for three months then $10 a month for most use cases; T-Mobile and
Syniverse one-off fees; resubmission $15: Telnyx pricing, read 2026-10-24) is part of a
`register_us_a2p` request whose quote lists them in GBP; a person approves it. The EIN is in the profile's
sealed `tax_ids` and is opened only in the executor at submission (Pylota parity 10). The status view
`GET /v1/tenants/{tenant_id}/phone-registrations` returns
`{configured, brand_status, campaign_status, numbers_registered[], failure_reasons[]}` (Pylota parity 10);
the enforce switch is the send gate itself and cannot be turned off for US senders. A tenant registers once:
every US long code it buys later is assigned to its campaign by the configure step, so `outbound_sms` of a
new number goes from `activating` to `enabled` without another person's decision (one registration per
customer, as AgentPhone's `POST /v1/register` is: npm `agentphone` 1.0.25, `reference.md`, Registration, read
2026-10-26). The fees of a `register_us_a2p` hold count against `phone.spend.registration_cap_pence`, not the
monthly cap ([§18.3](#183-spend-caps)). An update to an approved registration (new sample messages, a new
website) is a new `register_us_a2p` request with no brand fee, and the numbers keep sending under the current
approval while the update is reviewed.

**Toll-free verification** (`verify_toll_free` request): business identity, use case, volume, samples and
opt-in description, and the business registration fields (`businessRegistrationNumber`, `Type`,
`Country`; `Companies House` accepted). Statuses `In Progress`, `Waiting For Telnyx`, `Waiting For Vendor`,
`Waiting For Customer` (action needed), `Verified` (ready), `Rejected` (failed, with the reason). Review
takes 1–3 business days to start and 5–10 with the carriers (Telnyx toll-free verification guide, read
2026-10-24). **Conflict**: the schema says the registration fields are required from January 2026, the guide
says 17 February 2026; both dates have passed, so they are always sent.

### 13.1 US sole-proprietor brands

The Campaign Registry has a `SOLE_PROPRIETOR` brand type for a person trading without an EIN. Telnyx's API
creates it like any brand and then verifies it with a one-time code sent by text to the proprietor's mobile
number: "Only allowed for Sole Proprietor (`SOLE_PROPRIETOR`) brands … Campaigns cannot be created until OTP
verification is complete … US/CA numbers only for real OTPs" (Telnyx OpenAPI `team-telnyx/openapi`
`openapi/spec3.json` at `aeedb6553ea4`, `/10dlc/brand/{brandId}/smsOtp`, read 2026-10-26). AgentPhone sells the
same type with a limit of one number per registration (npm `agentphone` 1.0.25, `RegistrationRequest`, read
2026-10-26). **Not offered:** the owner refused sole traders ([§1.1](#11-sole-traders-refused-owner-decision-2026-10-26),
option (c)), so no sole-proprietor brand is registered and M39 does not build this path. It stays specified so a
later decision for options (a) or (b) can switch it on; it would then work as follows:

1. The profile's `entity_type` is `sole_trader`, `registration_country` `US`, and its representative is the
   proprietor, whose mobile must be a US or Canadian number (`422 compliance_incomplete`,
   `details.reason = "otp_mobile_country"`).
2. The `register_us_a2p` request creates the brand, then triggers the code. The code goes to the proprietor's
   own handset, never to an agent or a number the tenant holds here: a person types it into the console's
   **Phone › Setup** page, which calls `POST /v1/tenants/{tenant_id}/phone-registrations/{registration_id}/otp`
   (`phone:compliance`, a decision reserved for people; `{ "pin" }`, never logged or stored). A code that is
   wrong or expired answers `422 phone_registration_otp_invalid`; `…/otp/resend` triggers a new one, at most
   5 times a day.
3. The tenant's US numbers are limited to one with `outbound_sms` (`phone_number_limit_reached`,
   `details.limit = "sole_proprietor_sms"`); others may receive.

`live::phone::sole_prop_otp` runs the flow on staging with a mock brand (the spec allows non-US numbers for
mock brands) before the path is offered.

### 13.2 Brands registered outside the US

A tenant registered in the UK that buys a US number needs a brand whose country is GB. Telnyx's brand schema
takes `country` as any ISO 3166-1 alpha-2 code and requires `ein` only for non-profits (`CreateBrand`, Telnyx
OpenAPI at `aeedb6553ea4`, read 2026-10-26), so the API accepts such a brand; whether The Campaign Registry and
the carriers vet one, and which identifier they want in place of an EIN (the Companies House number is the
candidate), was not found in what was read. Until `live::phone::uk_brand_registration` has registered a
GB-registered business's brand and campaign on staging and counsel has answered question 11
([§32](#32-questions-for-counsel)):

- the catalogue's `us_a2p.brand_countries` is `["US"]`;
- a US number bought by a tenant registered elsewhere reports `outbound_sms: not_available` with
  `reason = "brand_country_not_supported"`, its quote's `follow_on` says so, and the one-call route needs
  `acknowledge_not_available: true` to buy it ([§6.7](#67-one-call-purchase));
- `register_us_a2p` from such a tenant answers `422 compliance_incomplete` with
  `details.reason = "brand_country_not_supported"`.

When both are done, a catalogue release adds `GB` to `brand_countries` and maps the identifier.

## 14. Voice

### 14.1 Routes: what v1 offers and what waits

Each number with the `voice` capability has one **voice route** (`phone_voice_routes`, `vrt_`). Setting
or changing it is a `set_voice_route` request a person approves, because a route decides who hears a
business's callers and a wrong one diverts them.

| Kind | v1 | What it does | Why |
|---|---|---|---|
| `vapi` | **Yes** | Calls go to the tenant's Vapi organisation through Vapi's bring-your-own SIP trunk; outbound calls are placed through Vapi | Parity with Pylota, whose calls all run through Vapi today (parity 7) |
| `sip` | **Yes**, inbound only | Calls go to a SIP URI the tenant names (its own PBX, LiveKit, another voice runtime) | The simplest route for a customer with its own voice stack; inbound needs no credential leaving Pylota Identity |
| `forward` | **Yes** | Calls are forwarded to an E.164 number (a person's phone, a branch) | Human handoff without a voice platform; Pylota's operators already forward with GSM divert codes |
| `none` | Yes | Calls are rejected | The default for a new number until a route is approved |
| `call_control` | **Later (v1.1)** | Pylota Identity receives Call Control webhooks and exposes call commands; media streams go straight from the provider to the customer's WebSocket | Deferred: Telnyx tells clients to resend a command when no response arrives within 500 ms, which a relay through the Worker and a Durable Object would often miss; it needs about thirty command routes wrapped, priced and permission-checked; recording consent and disclosure need a call state machine Pylota Identity owns; media streaming bypasses Pylota Identity, so only call set-up would be governed; and Pylota does not use it today |

Telnyx AI Assistants are not offered as a route: Telnyx sells agent identity itself, and Pylota's AI stays
in Pylota's own runtime (Telnyx study, recommendation 6).

### 14.2 `vapi`

The tenant registers a **voice connector** (`phone_voice_connectors`, `pvc_`): its Vapi private API key,
**sealed** (`api_key_sealed`), and its Vapi region (`us` or `eu`). Registering, rotating and deleting a
connector needs `phone:manage`; the key is shown never again (a one-time secret, [Security § 6.1](security.md#61-inventory)).
The route's configuration is `{connector_id, assistant_id | squad_id, server_url, server_secret}`:
`server_url` is the integrator's endpoint for Vapi's server messages (for Pylota, its own Vapi webhook endpoint), and
`server_secret` (sealed) is sent by Vapi as `X-Vapi-Secret` (Pylota parity 7).

Provisioning, in the executor (Vapi Telnyx SIP guide and `POST /phone-number` reference, read 2026-10-24):

1. **Vapi credential, once per connector.** `POST https://api.vapi.ai/credential` with
   `provider: "byo-sip-trunk"`, the Telnyx gateway IP addresses for inbound (Vapi refuses host names in
   `gateways`), and an outbound authentication plan with the tenant's credential-connection SIP user name and
   password ([§8](#8-tenant-provider-resources)). The Vapi credential ID is stored on the connector. Pylota
   Identity never gives Vapi a Telnyx API key: Vapi holds only the tenant's SIP credentials, which reach one
   connection bounded by the tenant's outbound voice profile.
2. **Telnyx side.** Assign the number to the tenant's credential connection and set its voice translated
   number to the Vapi SIP URI (`sip:{number}@{credential_id}.sip.vapi.ai`, or `sip.eu.vapi.ai` for an EU
   organisation), read back.
3. **Vapi number.** Adopt an existing Vapi phone number with the exact E.164 (Pylota parity 7), or create
   one with `POST /phone-number` (`provider: "byo-phone-number"`, `number`, `credentialId`,
   `numberE164CheckEnabled: false`, the assistant or squad, and `server {url, headers: {"X-Vapi-Secret": …}}`).
4. **Verify.** `PATCH` the Vapi number to the route's values and `GET` it back; every field must match
   (Pylota parity 7, PATCH-then-GET). Store `vapi_phone_number_id` on the route and return it to the
   integrator, which needs it for Vapi calls it makes itself.

`voice_status` becomes `ready` only after step 4. A Vapi `401` makes the route `failed` with
`voice_configuration` and notifies the tenant (its key was revoked). Pylota's current Vapi numbers use
Vapi's `telnyx` provider type with a credential created from Pylota's own Telnyx key; adoption records them
as they are, the first route change re-creates them as `byo-phone-number`, and the old Vapi credential and
the Telnyx key behind it are retired by the owner after migration ([§31](#31-pylota-agentic-os-parity-and-migration)).
What Vapi's `telnyx` credential type contains is not stated in the reference read; nothing in this design
depends on it.

### 14.3 `sip`

Configuration: `{uri, transport: "udp" | "tcp" | "tls"}`, the URI a host name, never an IP literal in a
private range ([Security § 9.1](security.md#91-url-rules-at-create-update-and-every-attempt) rules 2 and 3).
The executor points the number at an FQDN connection for that host (one per tenant and host, tagged), and
verifies by reading it back; the first inbound call's `call.initiated` confirms it end to end. v1 gives the
tenant no SIP credentials, so a `sip` route cannot place outbound calls; outbound SIP arrives with
`call_control`.

### 14.4 `forward`

Configuration: `{to: E.164, timeout_seconds: 10–60}`. The destination's country must be in
`phone.calls.allowed_destination_countries`; it must not be a premium-rate or emergency number; and it must
not be one of the tenant's own numbers (a loop). The executor sets the number's call forwarding at the
provider and reads it back (the exact Telnyx voice-settings fields are checked by
`live::phone::forwarding`). Each forwarded call costs inbound plus outbound minutes, priced in the quote.

### 14.5 Outbound calls

`POST /v1/identities/{identity_id}/calls` (`phone:call`; `Idempotency-Key` required):

```json
{ "from": "pnm_01JB…", "to": "+447700900456", "purpose": "Confirm the 14:00 collection for BK-2291",
  "assistant": { "assistant_id": "asst_…", "variables": { "booking": "BK-2291" } } }
```

1. **Route.** v1 places calls only through a `vapi` route (`POST https://api.vapi.ai/call` with
   `phoneNumberId`, `customer.number` and the assistant): other routes get `409 voice_route_unsupported`.
2. **Destination.** Allowed countries only; emergency, short and premium-rate numbers are refused
   (`403 destination_blocked`, from the catalogue's `emergency.numbers` and a compiled premium-prefix list
   per country).
3. **Policy.** `phone.calls.outbound`: `off` refuses (`403 policy_denied`, `details.reason = "calls_off"`);
   `approval` (the default) creates an `outbound_call` request whose digest covers `from`, `to`, `purpose`
   and the assistant, and answers `202` with the request; `allowed` places the call now. Pylota's three
   approval-gated call kinds (renter or insurer, vendor, seller) keep working under `approval` (parity 7).
4. **Disclosure.** When `phone.disclosure.calls` is `required` (the default, a guard field), the call's
   first spoken words are the tenant's disclosure text (`phone.disclosure.call_text`, default "This is an
   automated assistant calling for {brand}."), set as the assistant's first message override; a call
   whose route cannot carry the override is refused (`409 voice_route_unsupported`, `details.reason =
   "disclosure"`).
5. **Spend.** A hold for the call's maximum cost (`phone.calls.max_minutes`, default 15, at the
   destination's rate); `402 spend_cap_reached` when refused.
6. **Place.** Vapi's create-call route has no idempotency key, so the run writes a spend intent and, on an
   unknown outcome, the call becomes `uncertain`; the reconciler lists the Vapi number's calls created in
   the minute around the attempt with the same customer number (verified by `live::phone::vapi_call_list`)
   and adopts or proves absence before anything else happens. A call is never placed twice by a retry.

Calls placed by the tenant's own Vapi organisation without Pylota Identity (with its own Vapi key) are outside
this path. They are bounded at the provider by the tenant's outbound voice profile (allowed countries,
daily spend, concurrency), and the nightly reconciler compares outbound call records with the calls
Pylota Identity placed: a call it did not place raises `phone_unplaced_outbound_call` and notifies the tenant
([§17](#17-reconciliation)).

### 14.6 Recording, transcripts and disclosure

- **Recording is off by default.** `phone.recording` is `off` or `with_announcement` (a guard field). Pylota
  Identity records nothing itself in v1. A `vapi` route declares whether its assistant records; a route may
  declare recording only when the policy is `with_announcement`, and then the disclosure text gains the
  recording announcement ("This call is recorded.", `phone.recording.announcement_text`). The reconciler
  checks Vapi's call records for recordings on routes that declared none and raises
  `phone_recording_undeclared`.
- **Transcripts are untrusted input.** In v1 transcripts flow from Vapi to the integrator, not through
  Pylota Identity. Whatever transcript Pylota Identity stores later (Call Control, v1.1) is fenced and
  flagged untrusted and screened like an inbound SMS ([§10.3](#103-screening)). The guide tells integrators to
  treat caller speech the same way.
- **AI disclosure** is on by default for calls and first SMS. EU AI Act Article 50(1) requires a system that
  interacts with people to inform them they are interacting with an AI unless that is obvious, from
  2026-08-02 (artificialintelligenceact.eu/article/50, read 2026-10-24); the FCC's Declaratory Ruling 24-17
  treats AI-generated voices as "artificial voice" under the TCPA (FCC-24-17A1, read 2026-10-24). Whether
  the defaults are enough, and whether a tenant may turn them off, is for counsel ([§32](#32-questions-for-counsel)).

### 14.7 Call records and cost

The tenant's credential connection sends call events to `/hooks/telnyx` with call cost
(`call_cost_in_webhooks: true`). The consumer records each call in the identity's mailbox (`calls`):
direction, number, counterparty, route kind, start, answer and end times, duration, outcome, cost, and
whether disclosure applied. `call.started` and `call.ended` are emitted. Calls the webhooks missed are filled
from call detail records by the nightly reconciler (`source: "cdr"`). Usage per number and month (calls,
voice minutes, SMS segments in and out) is served by `GET /v1/identities/{identity_id}/phone-usage`
(`phone:read`), counting exactly what the caps count (Pylota parity 12).

### 14.8 Emergency calling

Each catalogue row has an `emergency` setting. In v1:

- **United States**: a number that places calls needs a registered emergency address, or Telnyx treats its
  emergency calls as unregistered and charges a $100 penalty (Telnyx E911 setup guide, read 2026-10-24).
  An `enable_emergency` request (validated address from the profile, $1.50 a month) registers it; until then
  the number's routes never place calls. The address must be in the US: E911 routes a caller to the
  emergency service for the registered location. A tenant registered outside the US whose profile holds no
  verified US address therefore cannot place calls from a US number: `outbound_calls` is `not_available`
  with `reason = "no_us_emergency_address"`, inbound calls work, and `enable_emergency` with a non-US
  address answers `422 compliance_address_invalid` (`details.reason = "emergency_address_country"`). Whether
  Telnyx offers any registration for such a tenant is a question for Telnyx ([§39](#39-open-questions-for-the-owner),
  question 6), checked by `live::phone::e911_address`.
- **United Kingdom and EU countries**: how emergency address registration works for these numbers is not
  documented in what was read (unknown 7.3.2 of the study). Outbound calls from agent numbers never dial
  emergency numbers (refused at step 2 of [§14.5](#145-outbound-calls)), and the tenant is told, clearly and
  before purchase, that emergency calls are not supported from these numbers, as the reseller annex requires
  "enable local emergency numbers … or clearly warn if not supported" with prior notice (Telnyx reseller
  requirements annex, read 2026-10-24). The warning is part of the quote and of the console's purchase page.
- Whether a warning suffices for numbers that can make outbound calls, and what TREFT's obligations are, is
  question 2 for counsel. A row's emergency setting changes only by a catalogue release.

## 15. Porting in

A `port_in` request moves numbers the tenant already has into one of its identities:

1. **Check.** `POST /v1/identities/{identity_id}/phone-numbers/portability` (`phone:request`; free and
   read-only, so it needs no approval) runs the provider's portability check and returns per number
   `portable`, `fast_portable`, `messaging_capable` and the reason when not portable.
2. **Request.** `port_in` with the numbers, the losing carrier's account details (account number, PIN or
   password; **sealed** in the request, cleared once the port completes or fails), the authorised signer, a
   requested FOC date, and the compliance action `porting` for the numbers' rows (the profile must be
   approved for it). The quote lists the provider charges a port can incur (none per number; a GB order
   change $30 and rejection $18: Telnyx pricing, read 2026-10-24).
3. **Approve.** A person approves; the losing-carrier secrets are written by a `phone:compliance` key or the
   console, never by an identity key.
4. **Execute.** `POST /porting_orders` creates drafts, which the provider may split by country, type and
   carrier; each draft is a `porting_in` number row and a `pti_` record under the request. For each: `PATCH`
   the end user, service address, requirements (the letter of authorisation generated from the profile and a
   recent invoice, uploaded and linked in one run, [§4.3](#43-requirement-groups-and-submission)), the
   tenant's messaging profile and connection, and a FOC date from the allowed windows; then confirm.
   `customer_reference = "pi:{request_id}:{attempt}"` brackets the create as for orders
   ([§6.3](#63-spend-intent-customer_reference-and-uncertain)).
5. **Track.** `draft`, `in-process`, `submitted`, `foc-date-confirmed` keep the number `porting_in`;
   `exception` makes the request `action_needed` with the provider's comments (untrusted) and notifies the
   tenant; `ported` moves the number to `configuring`, then `active`; `cancel-pending` and `cancelled` end it
   as `failed`. Cancelling through the API is not possible within 48 hours of the FOC date (Telnyx porting
   guide); a cancel request then gets `409 port_in_not_cancelable`.
6. **Outside the US and Canada voice and SMS port together**; for US numbers messaging can port separately
   and later, so `sms_status` stays `pending` until the messaging port completes.

## 16. The port-out guard

A port-out is the phone equivalent of a SIM swap: another carrier asks for the number, and if nobody answers
within 24–48 hours the provider authorises it automatically (Telnyx port-out guide, read 2026-10-24). An
agent's number could leave in two days unnoticed. The guard:

1. **A PIN on every number.** At activation the configure step generates an `external_pin` (8 digits from
   `platform::Rng`; the provider's format rules are checked by `live::phone::external_pin`), sets it on the
   number, reads the number back, and stores the value **in the vault** as a managed secret owned by the
   number (`put_managed(tenant_id, identity_id, owner_ref = number_id, ManagedPurpose::PortOutPin, value)`,
   [Vault § 13.2](vault.md#132-managed-secrets)); it is never in D1, a log or a response, and the
   plaintext is zeroised after the call. A gaining carrier's request without the PIN gives the tenant a
   strong reason to reject. Sealing a managed secret needs only the tenant vault's public key, so this works
   in every vault custody mode.
2. **Notice at once.** `portout.status_changed` with `pending` (or a pending port-out found by the
   reconciler, which lists `GET /portouts?filter[status]=pending` every 15 minutes as a backstop for a missed
   event) creates a `phone_port_outs` row (`pto_`) with `respond_by` = the event time + 24 hours (the short
   end of the window), moves the number to `port_out_pending`, emits `phone_number.port_out_requested`,
   sends an urgent account email to the tenant's owners and admins and to the identity's accountable human,
   puts it first in "Needs a person", and raises the page alert `phone_port_out_pending` for the operator.
3. **A person decides.** `POST …/port-outs/{port_out_id}/authorize` or `…/reject` (with a reason code from
   the provider's list for this port-out; code 1001 "Other" needs text) is a decision reserved for people
   (`Decision::DecidePortOut`, under `phone.key_approval` as for requests). Authorising lets the number go;
   rejecting refuses a subscriber's move. Both are audited with the decider and `on_behalf_of`.
4. **Never rejected automatically.** Number portability is the subscriber's right and Telnyx's porting team
   reviews every rejection (`rejected-pending`), so no rule, timer or agent rejects. If nobody decides, the
   guard escalates at 50%, 75% and 90% of the window (`phone_port_out_deadline` alerts and repeated emails),
   and the provider's automatic authorisation then completes the port.
5. **After.** `ported`: the number becomes `ported_out`, its route is removed, its vault PIN deleted,
   charges stop, and the identity keeps its history. `rejected`, `canceled`: back to `active`, and the PIN is
   rotated (a new value set at the provider and stored in the vault), because a rejected request may mean the
   PIN leaked.

**The phone service never reads a PIN back.** Every change of PIN (after a rejected port-out, after a reveal,
or when the reconciler finds the provider's PIN missing or different) generates a new value, sets it at the
provider, reads the number back, and replaces the managed secret with `put_managed`; the old value is never
needed. So the phone service does not call `use_managed`, and a tenant whose vault is in partner custody (where
the Worker cannot open a managed secret without the partner's unlock key) loses nothing. `delete_managed` runs
at release and when a port-out completes.

**When the tenant needs the PIN** to port its own number away, a person reveals it in the console
([Vault § 13.1](vault.md#131-human-reveal-p1)): owners and admins, signed in within 10 minutes. Where the vault
cannot reveal (partner custody), the console's **Port PIN** action on the number instead rotates the PIN and
shows the new value once to that person, on a page served with `Cache-Control: no-store`, with the audit row
`phone_number.pin_shown`. After either, the PIN is rotated again 72 hours later unless a port-out is pending.
No API, MCP or CLI route shows a PIN. Port-out rules and
timings in the UK and EU are not documented by the provider (unknown 7.3.12), so the 24-hour floor stands
until a contract test or the provider says otherwise.

## 17. Reconciliation

Telnyx retries little (messaging webhooks make 6 attempts; voice failover after two) and its documents
conflict, so the provider's state is read back on a schedule, not only on events.

**Continuous** (the every-minute cron, [§6.6](#66-who-moves-an-order)): due requests and numbers, port-out
listing every 15 minutes, uncertain attempts on their schedule, tenant resources with `sync_due_at`.

**Nightly** (`jobs/phone_reconcile.rs`, started by the daily cron at 02:30 UTC as a `JobRunner` job per
provider account, resumable by step):

| Step | Reads | Repairs and reports |
|---|---|---|
| `numbers` | Every page of `GET /phone_numbers` (complete listing required) | A live row missing at the provider: `suspended` (`missing_at_provider`) and `phone_reconcile_drift`. A provider number with Pylota Identity's tag and no row: `phone_orphan_number` (a cost leak; never deleted automatically). A number whose messaging profile, connection, translated number or PIN presence differs: re-configured with read-back |
| `orders` | Each order of a non-final request | State transitions as from events |
| `porting` | Porting orders and port-outs in flight; republish of missed porting events | As from events |
| `compliance` | Requirement groups of every profile | Status and expiry; notifications |
| `a2p` | Brands, campaigns, number assignments, toll-free verifications | Status, readiness of numbers |
| `messages` | Message detail records of the previous UTC day (`GET /detail_records`) | Final statuses for SMS without one; `uncertain` matches; provider cost versus charges (`phone_charge_drift_pence`) |
| `calls` | Call detail records of the previous day | Missing `calls` rows; outbound calls Pylota Identity did not place ([§14.5](#145-outbound-calls)) |
| `deliveries` | `GET /webhook_deliveries` failed attempts to `/hooks/telnyx` of the previous day | For an event ID not in `phone_provider_events`, the owning resource is re-read (or the event republished where the provider allows it); `phone_webhook_missed_total` |
| `optouts` | `GET /messaging_optouts` per tenant profile | Ledger entries missing on either side, reported; a provider opt-out absent from the ledger is added (`provider_sync`) |
| `balance` | `GET /balance` | `phone_provider_balance` metric and alert ([§18.4](#184-provider-side-backstops-and-balance)) |
| `report` | – | Counts per step in `jobs.result_json`; `phone_reconcile_drift` when any money-affecting drift is non-zero |

**On request.** `POST /v1/tenants/{tenant_id}/phone/reconcile` (`phone:manage`, at most once every 10 minutes
per tenant) runs the `numbers`, `orders`, `porting` and `a2p` steps for one tenant; `POST
/v1/platform/phone/reconcile` (`platform:ops`) runs the nightly job now.

## 18. Money

### 18.1 Prices, currency and the rate card

Telnyx bills in **US dollars**: its API returns only USD and its site offers USD and EUR, converted daily
(Telnyx pricing pages and pricing API, read 2026-10-24). Pylota Identity Cloud bills its customers in
**pounds sterling** (PRD § 13). Prices are therefore a published **rate card**, not the provider's cost at
the moment: `crates/core/data/phone-rate-card.json`, overridable with `PM_PHONE_RATE_CARD`, versioned
(`version`), with one row per catalogue `rate_card_key` and per usage item. Each GBP price is generated by
`cargo xtask phone-rate-card` from the provider's USD list price:

```text
price_pence = ceil( usd × fx × (1 + fx_buffer) × (1 + margin) × 100 )
fx         = 0.748   (1 USD in GBP, ECB reference rates of 2026-10-23)
fx_buffer  = 0.10    (exchange-rate headroom)
margin     = 0.40    (owner decision, 2026-10-26)
```

Where Telnyx's pages give two prices for one item, the card uses the higher (UK SMS $0.050 or $0.055:
$0.055). Regulatory pass-through fees (10DLC brand, vetting, campaign and resubmission fees) are priced at
cost, `ceil(usd × fx × (1 + fx_buffer) × 100)`, without the margin (owner decision, 2026-10-26): brand and campaign up front $34.50 → £28.39; the
campaign's $10 a month → £8.23. Rows with the owner's 40% margin and 10% currency buffer (decided 2026-10-26):

| Item | Telnyx USD (2026-10-24) | GBP price |
|---|---|---|
| GB mobile number, monthly / one-time | $2.00 / $2.00 | £2.31 / £2.31 |
| GB local number, monthly / one-time | $1.00 / $1.00 | £1.16 / £1.16 |
| US local number, monthly (plus SMS enablement $0.10) | $1.10 | £1.27 |
| US toll-free number, monthly | $1.00 | £1.16 |
| IE local number, monthly | $0.50 | £0.58 |
| NL mobile number, monthly / one-time | $3.00 / $2.00 | £3.46 / £2.31 |
| Outbound SMS to GB, per segment | $0.055 | £0.07 |
| Outbound SMS to US from a long code, per segment (plus carrier fee up to $0.0045) | $0.0085 | £0.01 |
| Inbound SMS in GB, IE, NL | $0 | £0 |
| Inbound voice on a GB mobile number, per minute (with the $0.002 platform fee) | $0.0052 | £0.01 |
| US E911, monthly | $1.50 | £1.73 |

A number's monthly and one-time prices are fixed in its quote for the life of the number; a card change
applies to new quotes, and to existing numbers only after 30 days' notice by email to the tenant's owners
(owner decision). `phone_fx_drift` alerts the operator when the ECB reference rate (fetched daily from the
fixed host `www.ecb.europa.eu`) moves more than 5% from the card's `fx`.

### 18.2 What is charged

Each charge is a row of `phone_charges` (`chg_`), in pence, with the provider cost in USD as a decimal
string, the card version, the month (`YYYY-MM`, UTC) and a unique `source` (the provider event ID, or
`monthly:{number_id}:{YYYY-MM}`), so a repeated event charges once ([P75](#34-edge-cases)):

| Kind | When | Price |
|---|---|---|
| `number_setup`, `number_monthly` | At activation (setup and the first month), then on the 1st of each UTC month while the number is live (no proration; a month started is charged) | The quote |
| `sms_out`, `sms_in`, `mms_in` | At the receipt that carries the provider's `cost` | The card's price per segment for the destination or origin |
| `voice_in`, `voice_out`, `voice_forward` | At `call.ended` (or the call detail record) | The card's per-minute price, rounded up to the minute |
| `a2p_fee`, `emergency_monthly`, `port_fee` | When the provider charges it | The quote |

Search, the catalogue, reads, waits and inbound events are not charged.

### 18.3 Spend caps

Two caps per UTC month, enforced in the tenant's `TenantQuota` with holds, as plan allowances are
([Billing › TenantQuota allowances and holds](billing.md#tenantquota-allowances-and-holds)):

- `phone.spend.tenant_monthly_cap_pence` (lower-only; Cloud default 2,000, £20) and
- `phone.spend.identity_monthly_cap_pence` (lower-only; Cloud default 1,000, £10).

And one cap for one-off registration fees, which a monthly cap sized for numbers and texts would otherwise
block for good (the up-front 10DLC fees alone, $34.50, are £28.39 at cost through the card's exchange rate and
buffer, above the £20 tenant default):

- `phone.spend.registration_cap_pence` (lower-only; Cloud default 6,000, £60; `null`, no cap, in the built-in
  defaults), per UTC month, counted in `phone_spend:reg:{YYYY-MM}`. The holds of `register_us_a2p` and
  `verify_toll_free` count against it **instead of** the tenant and identity caps; the campaign's recurring
  monthly fee is a recurring charge like a number's, never refused ([§13](#13-us-a2p-10dlc-and-toll-free-verification)).

Every billable action takes a `phone_spend` hold of its estimated price before it runs (`Hold {feature:
"phone_spend", units: pence, ref}` against both caps in one transaction), settles at the actual charge, and
releases the rest. A refused hold answers `402 spend_cap_reached` with `details.scope` (`tenant`,
`identity` or `registration`), `cap_pence`, `used_pence` and `resets_at`, before any idempotency record or row is written, so
the same key works after a person raises the cap ([P73](#34-edge-cases)). Monthly number charges are never
refused (refusing would lose the number): they are counted, and a tenant over its cap from recurring
charges alone gets `phone_spend.warning` at 100% and an email, but keeps its numbers. `phone_spend.warning`
is also sent at 80%. The holds use the existing `holds` table with a new feature value; the counters are
`phone_spend:t:{YYYY-MM}` and `phone_spend:i:{identity_id}:{YYYY-MM}`.

### 18.4 Provider-side backstops and balance

Pylota Identity's caps act first. Below them, at the provider: each tenant's messaging profile and outbound
voice profile carry a `daily_spend_limit` in USD of 1.5 times the tenant's daily SMS cap at its highest
destination price (and the call equivalent), converted at the card's `fx`; the account's per-product spend
limits are set by the operator at 1.5 times the sum of tenant caps. A provider refusal for these limits
(40333) fails the send with `reason: "provider_spend_limit"` and raises `phone_provider_limit_hit`, which
means the caps and the backstops are out of step.

The provider account is prepaid. The nightly step and an hourly check read `GET /balance`; `phone_provider_balance_low`
pages the operator when the balance covers fewer than 7 days of the last 7 days' average spend, and below 2
days the ops switch moves to `paused_purchases` automatically (new orders, ports and A2P fees refused with
`503 unavailable`, `details.reason = "provider_balance"`), while SMS, calls and inbound continue (Pylota
parity 12: low-balance alerting). An order refused for funds (`402`, 20100) is `carrier_billing`.

### 18.5 Invoicing

- **Plan required.** On a deployment with `PM_BILLING=stripe`, phone is available to a tenant on a paid plan
  with a Stripe customer, or to an `exempt` tenant of a partner (whose partner pays). A Free-plan tenant gets
  `403 policy_denied` with `details.reason = "plan_required"` on search and requests (owner decision).
- **Metered workspaces.** On the 1st of each month the invoicing job (`phone/invoicing.rs`, part of the
  daily cron) sums the previous month's charges per tenant and category and creates one Stripe invoice item
  per category on the tenant's Stripe customer, in GBP pence, with the Stripe idempotency key
  `pi-phone-{tenant_id}-{YYYY-MM}-{category}`; invoice items are "applied on the next invoice at the
  beginning of the next billing cycle" (Stripe "Query billing data", invoice items, read 2026-10-24). The
  restricted Stripe key gains the invoice-item write permission ([Security § 6.1](security.md#61-inventory)).
  Whether the current Stripe API takes `customer` or `customer_account` on `POST /v1/invoiceitems` is checked
  at build time (the documents read show both).
- **Partner tenants.** `GET /v1/partners/{partner_id}/phone-statement?month=YYYY-MM` (partner key with
  `phone:read`, or platform key) returns the month's charges per tenant and category; the partner is
  invoiced outside Stripe in v1. Pylota is TREFT's own product, so its statement is an internal record.
- **Billing off.** With `PM_BILLING=off` (self-hosting), charges are still recorded and shown, so caps and
  usage work, and nothing is invoiced; the self-hoster pays its own provider account.
- **Usage view.** `GET /v1/tenants/{tenant_id}/phone-charges?month=` (`phone:read`, tenant level and
  above) and the console's Phone › Charges page show charges, caps and holds.

## 19. Policy

New fields of the [tenant policy](../../reference/configuration.md#tenant-policy), each with its class
([Workspace policy § 2](workspace-policy.md#2-classes-and-ceilings)):

| Field | Default | Class | Meaning |
|---|---|---|---|
| `phone.allowed` | `false` | Lower-only (`false` is stricter) | Phone numbers may be used by the tenant. On Pylota Identity Cloud only a platform or the tenant's partner key turns it on |
| `phone.key_approval` | `false` | Platform or own partner | A platform key or the tenant's partner key may approve phone requests and decide port-outs ([§5.4](#54-approval)) |
| `phone.countries` | every catalogue country | Lower-only (fewer is stricter) | Countries the tenant may buy in |
| `phone.max_numbers_per_identity`, `phone.max_numbers_per_tenant` | 5, 20 | Lower-only | [§7.2](#72-roles-moves-and-limits) |
| `phone.spend.tenant_monthly_cap_pence`, `phone.spend.identity_monthly_cap_pence` | 2,000, 1,000 on Cloud; `null` (no cap) in the built-in defaults | Lower-only | [§18.3](#183-spend-caps) |
| `phone.spend.registration_cap_pence` | 6,000 on Cloud; `null` in the built-in defaults | Lower-only | One-off registration fees ([§18.3](#183-spend-caps)) |
| `phone.auto_approve` | `{ "enabled": false }` | Guard (setting or loosening is a decision reserved for people; tightening needs `phone:manage`) | The standing purchase budget ([§5.7](#57-the-standing-purchase-budget)) |
| `phone.sms.identity_daily_cap`, `phone.sms.tenant_daily_cap`, `phone.sms.tenant_monthly_cap` | 200, 2,000, `null` | Lower-only | Segments; [§12.2](#122-policy-pipeline) step 11 |
| `phone.sms.allowed_destination_countries` | the tenant's catalogue countries | Lower-only | Step 6; also the messaging profile's allowed destinations |
| `phone.sms.brand_name` | the profile's trading name | Free | Used in auto-responses and disclosures |
| `phone.inbound.per_sender_per_hour`, `phone.inbound.per_identity_per_hour` | 30, 600 | Free | [§10.2](#102-quarantine-rules-for-sms) |
| `phone.calls.outbound` | `approval` | Guard (`off` < `approval` < `allowed`; tightening moves left) | [§14.5](#145-outbound-calls) |
| `phone.calls.allowed_destination_countries` | the tenant's catalogue countries | Lower-only | Calls and forwarding |
| `phone.calls.concurrent_limit`, `phone.calls.max_minutes` | 2, 15 | Lower-only | The outbound voice profile and the call hold |
| `phone.recording` | `off` | Guard (`off` is stricter) | [§14.6](#146-recording-transcripts-and-disclosure) |
| `phone.disclosure.calls`, `phone.disclosure.first_sms` | `required`, `required` | Guard (`required` is stricter) | [§14.5](#145-outbound-calls), [§12.2](#122-policy-pipeline) step 9 |
| `phone.disclosure.call_text`, `phone.disclosure.sms_text`, `phone.recording.announcement_text` | as given above | Free (each 1–300 characters; `{brand}` is replaced) | |
| `phone.verification.redact` | `true` | Guard | [§11.5](#115-redaction) |

`accounts.require_approval`, `quarantine.unsolicited_otp`, `screening.*`, the receive lists and the vault's
`vault.codes_into_fills_only` apply to SMS as they do to mail. A tenant key may loosen a guard field only under the rule of
[Workspace policy § 3](workspace-policy.md#3-decisions-reserved-for-people).

## 20. Permissions and key levels

| Permission | Allows | Key levels |
|---|---|---|
| `phone:read` | The catalogue; the setup status; the standing budget and its use; the identity's numbers, requests, SMS (with `quarantine:review` for held ones), media, calls, usage; `sms/wait`; the tenant's lookup, registrations view and charges (tenant level and above) | every level |
| `phone:request` | Search; the one-call purchase; create and cancel its own `purchase_number`, `port_in` (without secrets), `release_number` requests; portability checks; onboarding drafts; an earlier `expires_at` | every level |
| `phone:send` | Send SMS, resolve uncertain SMS, record an opt-out | every level |
| `phone:call` | Place calls or create `outbound_call` requests | every level |
| `phone:approve` | Approve and reject phone requests; authorise and reject port-outs (each a decision reserved for people). Includes `phone:read` | platform, partner, tenant |
| `phone:manage` | Number roles and moves; voice connectors; `set_voice_route` and `enable_emergency` requests; record opt-ins; tenant reconcile; tightening the standing budget; a later or no `expires_at` | platform, partner, tenant |
| `phone:compliance` | The compliance profile and documents; `submit_compliance`, `register_us_a2p`, `verify_toll_free` requests; losing-carrier details of a port-in | platform, partner, tenant |

An identity key acts only for its own identity. No permission returns a provider credential, a sealed
value or a KYC document. While a workspace is a provisional agent sign-up, none of the `phone:*`
permissions is effective ([Agent self-sign-up § 4.1](agent-signup.md#41-permissions); `403
permission_denied`, `details.reason = "signup_unverified"`).

## 21. Concurrency and idempotency

| Operation | Pylota Identity's own idempotency | Provider idempotency | Unknown outcome |
|---|---|---|---|
| Create a request | `Idempotency-Key` required; D1 `idempotency_records` (scoped per key and tenant) | – | Safe to retry with the same key |
| One-call purchase | `Idempotency-Key` required; the request it creates; the inline run holds the same claim as the queue | – | A replay returns the stored `201` or `202`; an inline run cut short continues in the queue |
| Standing budget slot | The `phone_budget:t:{YYYY-MM}` hold, taken with the spend hold in one `TenantQuota` transaction | – | Released when the request fails or is canceled |
| Approve, reject, cancel | `Idempotency-Key` required; the guarded state change | – | Safe to retry |
| Order a number | The claim, the spend intent, `customer_reference` per attempt | None | `uncertain`; reconciled before any new attempt ([§6.3](#63-spend-intent-customer_reference-and-uncertain)) |
| Tenant resources, addresses, documents, groups | The claim | None | The next run lists by name and tag before creating |
| Porting orders | The claim, `customer_reference` | None | `uncertain`; reconciled |
| 10DLC brand | The claim, spend intent, brand lookup before create | None | `uncertain`; reconciled |
| 10DLC campaign | The claim | `referenceId` unique | Retried with the same `referenceId` |
| Send SMS | `Idempotency-Key` required; mailbox `idempotency` row with `sms_id`; the transport claim | `Idempotency-Key: pi-sms-{sms_id}` | Repeated with the same provider key (once the contract test passes), then `uncertain` ([§12.3](#123-dispatch-provider-idempotency-and-uncertain)) |
| Place a call | `Idempotency-Key` required; spend intent | Vapi: none | `uncertain`; reconciled from Vapi's call list |
| Provider events | `phone_provider_events` unique on the event ID; `last_event_at` per resource | – | – |
| Charges | Unique `source` per charge | – | – |
| Stripe invoice items | – | Stripe idempotency key per tenant, month and category | Retried with the same key |

## 22. Failure handling

| Failure | Behaviour |
|---|---|
| Provider `429` | `Transient`: the run backs off by `retry-after` (or 30 s doubling, at most 10 minutes) and keeps its claim; a synchronous route (search) answers `429 upstream_rate_limited` |
| Provider `5xx` or timeout on a read | Retried up to 3 times in the run, then the run ends and the reconciler tries later; a synchronous read answers `502 upstream_error` |
| Provider `5xx` or timeout on a create or charge | `uncertain` ([§6.3](#63-spend-intent-customer_reference-and-uncertain)) |
| Provider `401`/`403` | `not_configured`; page alert `phone_provider_auth`; the ops switch moves to `paused_purchases` |
| Provider `10038`/`10039` (account level) | The request fails with `not_configured`, `details.reason = "account_level"`; alert |
| D1 unavailable | Webhooks still acknowledged (R2 and the queue only); the consumer retries; API routes answer `503 unavailable` |
| The vault unavailable | The configure step retries `put_managed`; the number stays `configuring` (its SMS and voice facets may already be ready) until the PIN is stored, and `phone_pin_missing` is raised after an hour. Withheld codes wait for the vault; nothing is released to agents instead |
| Vapi unavailable | Route provisioning retries for 1 hour, then `voice_configuration`; calls answer `502 upstream_error` before any spend intent, or become `uncertain` after it |
| A Worker dies mid-run | The claim lapses after 5 minutes; the next run resumes from the stored state, never repeating a step whose effect is recorded |

New error codes ([Errors](../../reference/errors.md)), each with `retryable` fixed by the code:

| HTTP | Code | Retryable | When |
|---|---|---|---|
| 400 | `phone_number_invalid` | no | Not a valid E.164 number |
| 402 | `spend_cap_reached` | no | A phone spend cap would be exceeded (`details.scope`, `cap_pence`, `used_pence`, `resets_at`) |
| 403 | `recipient_opted_out` | no | The recipient opted out of this tenant's texts |
| 403 | `consent_required` | no | A marketing SMS without a marketing opt-in |
| 403 | `a2p_not_registered` | no | A US sender without a ready 10DLC campaign or toll-free verification (`details.registration`, `details.status`) |
| 403 | `destination_blocked` | no | An emergency, short-code or premium-rate destination |
| 404 | `phone_number_not_found`, `phone_request_not_found`, `sms_not_found`, `call_not_found`, `phone_document_not_found`, `port_out_not_found`, `voice_connector_not_found` | no | Missing or outside the key's scope |
| 409 | `phone_number_not_ready` | yes while configuring | The number is not `active`, or lacks the capability (`details.reason`) |
| 409 | `phone_number_taken` | no | The exact number asked for was taken before the order (`details.alternatives[]`: up to 5 fresh candidates with quotes and quote tokens) |
| 409 | `phone_request_not_pending` | no | Approve, reject or cancel in a state that cannot be decided (`details.status`) |
| 409 | `phone_request_digest_mismatch` | no | The approval names a digest that is not current (`details.current_digest`) |
| 409 | `compliance_not_approved` | no | The tenant's profile is not approved for the row and action |
| 409 | `quiet_hours` | yes | A marketing SMS inside the destination's quiet hours (`details.next_allowed_at`) |
| 409 | `identity_has_phone_numbers` | no | Deleting an identity that holds a live number |
| 409 | `voice_route_unsupported` | no | The route cannot do what was asked (`details.reason`) |
| 409 | `phone_document_in_use` | no | Deleting a document a live provider group still needs |
| 409 | `port_in_not_cancelable` | no | Within 48 hours of the FOC date |
| 409 | `port_out_closed` | no | Deciding a port-out that is no longer pending |
| 410 | `phone_quote_expired` | no | A `quote_token` older than 30 minutes |
| 422 | `country_not_offered`, `local_presence_required` | no | [§3.3](#33-rules-the-catalogue-enforces) |
| 422 | `compliance_incomplete` | no | Missing profile fields or documents (`details.missing[]`), or an entity type v1 refuses |
| 422 | `phone_number_limit_reached`, `phone_request_limit_reached` | no | [§7.2](#72-roles-moves-and-limits), [§5.2](#52-the-request-and-its-states) |
| 422 | `phone_registration_otp_invalid` | no | A sole-proprietor brand's one-time code is wrong or expired ([§13.1](#131-us-sole-proprietor-brands)) |

Existing codes reused: `403 invalid_signature` on `/hooks/telnyx`; `403 policy_denied` with the new
`details.reason` values `phone_not_allowed`, `plan_required`, `destination_country`, `destination_untested`,
`destination_not_supported`, `calls_off`, `codes_into_fills_only`, `account_not_approved`; `400 invalid_request`
with `location_not_supported`; `402 spend_cap_reached` with `details.scope = "registration"`;
`409 phone_number_not_ready` with `legacy_unpapered`; `422 compliance_incomplete` with `entity_type`,
`otp_mobile_country` and `brand_country_not_supported`; `422 compliance_address_invalid` with
`emergency_address_country`; `422 phone_number_limit_reached` with `details.limit = "sole_proprietor_sms"`; `429 daily_cap_reached` with `details.cap` `phone_search`,
`sms_identity`, `sms_tenant`, `sms_tenant_month`; `502 upstream_error` and `503 unavailable` with
`details.reason` `provider`, `phone_paused`, `provider_balance`. SMS failures after `202` are statuses and
reasons, never HTTP errors, as for mail.

## 23. Security (T2)

**Why T2.** This feature spends money on the operator's prepaid account, holds KYC documents and identity
data, routes calls and texts between tenants on one shared provider account, and holds provider and
voice-platform credentials. Each of those is a T2 surface: the build needs a security
review, an independent adversarial check, the flag, telemetry and rollback of [§30](#30-rollout-telemetry-and-rollback),
and the owner's sign-off before merge ([ADR 0020](../adr/0020-phone-numbers.md)).

| Threat | Control |
|---|---|
| An agent spends money, ports, releases or submits KYC on its own, or is steered to by an SMS | Every such action is a request approved by a person (`key_may_decide`); identity keys cannot hold `phone:approve`, `phone:manage` or `phone:compliance`; a key never approves its own request ([§5](#5-phone-requests-approval-for-everything-that-costs-money-or-cannot-be-undone)) |
| An approval is replayed onto a different purchase | The approval names the digest of the exact payload and quote; a changed price re-opens approval |
| An agent raises its own standing budget, or a compromised key buys up to the budget | Setting or loosening `phone.auto_approve` is a decision reserved for people; identity keys never change it; the budget is bounded per number, per identity and per month, sits under the spend caps, is off by default, and is audited with its version on every purchase it covers; the ops switch and `enabled: false` stop it at once |
| An agent's drafted registration text carries instructions or links to the person who reviews it | Drafts are untrusted text, shown escaped, never rendered as links or markup; a person edits and submits; drafts never hold tax IDs, personal data or documents ([§4.7](#47-phone-onboarding)) |
| A sole-proprietor brand's one-time code reaches an agent | The code goes to the proprietor's own handset and is typed by a person into the console; no tool or route returns it ([§13.1](#131-us-sole-proprietor-brands)) |
| Provider credentials leak to tenants or agents | The Telnyx key is a Worker secret used only by the provider client; no route returns it; agents get narrow tools, never the provider's MCP server; Vapi receives SIP credentials of one tenant connection, never a Telnyx key |
| Cross-tenant routing on the shared account (an SMS, a receipt or an order event lands in the wrong tenant) | The owner comes from facts the provider controls, cross-checked with a second fact; a mismatch is dropped and alerted ([§9.4](#94-resolving-the-tenant)); one messaging profile, connection and billing group per tenant; ownership stamps checked before release |
| A forged webhook injects SMS, receipts, port-outs or order completions | Ed25519 over timestamp and raw body in every environment, a 5-minute window, deduplication by event ID; events trigger state reads where money moves (an order completion is confirmed with `GET /number_orders/{id}` before a number is activated) |
| A port-out takes a number silently | The port-out guard ([§16](#16-the-port-out-guard)) |
| SMS text steers an agent (prompt injection) | Marked untrusted, fenced for models, screened like mail; codes redacted; links in SMS never become verification objects ([§10](#10-inbound-sms), [§11](#11-verification-codes-over-sms)) |
| A forged SMS sender passes a verification gate | Senders are flagged unverified; the gate needs an approved ledger entry, a registered wait and, where listed, the sender ([§11.2](#112-the-gate)) |
| KYC documents leak | Encrypted per document with a sealed data key; write-only; no read route; a platform export only, audited; crypto-shredded on erasure ([§4.2](#42-documents)) |
| A voice route diverts callers to an attacker | Route changes are approved requests; `sip` URIs pass the SSRF host rules; `forward` destinations are checked |
| Number recycling delivers a previous holder's codes | 90-day quarantine across tenants; history scoped to the ownership period ([§7.5](#75-quarantine-of-released-numbers)) |
| Runaway spend (a loop, a compromised key, a bug) | Caps in `TenantQuota`; provider-side daily limits per tenant; balance alerts and the automatic pause; the ops switch ([§18](#18-money), [§30](#30-rollout-telemetry-and-rollback)) |
| SSRF through media URLs or SIP hosts | Media only from compiled provider hosts; SIP host names validated as webhook URLs are ([Security § 9](security.md#9-ssrf-controls)) |

**Secrets** added to [Security § 6.1](security.md#61-inventory): `PM_TELNYX_API_KEY` (provider API calls;
generated by the operator in the Telnyx portal; a leak lets someone buy numbers and send on the operator's
account until rotated), `PM_TELNYX_PUBLIC_KEY` (not secret; verifies webhooks), per-tenant SIP credentials
(`phone_tenants.sip_sealed`), Vapi keys (`phone_voice_connectors.api_key_sealed`) and server secrets
(`phone_voice_routes.server_secret_sealed`), KYC data keys (`phone_compliance_documents.key_sealed`), sealed
profile fields (`phone_compliance_profiles.tax_ids_sealed`, `representative_sealed`), and port-in
losing-carrier details (`phone_requests.secrets_sealed`). Each sealed column is an entry of
`crates/core/src/sealed.rs`, re-sealed by the master-key rotation. Port-out PINs live in the vault.

**Logs** hold no SMS text, no codes, no call content and no clear phone numbers: an E.164 that must be
correlated is logged as `HMAC-SHA256(PM_HASH_KEY, e164)` truncated to 16 hex characters, as addresses are
([Security § 12](security.md#12-logging-rules)). Provider error bodies are logged as their code only.

## 24. Privacy

| Store | Personal data | Retention | Deletion |
|---|---|---|---|
| R2 `phone-staging/…` | Raw provider events: SMS text, numbers | Until the consumer processes them; at most 15 days | Deleted after processing; lifecycle rule. Not keyed by tenant, so listed as a residual |
| Mailbox `sms_messages`, `sms_conversations`, `sms_media`, R2 media | Counterparty numbers, text, media | The tenant's `retention.message_days` and the identity's `retention.message_hours`, as mail | Retention; message, conversation, counterparty, identity and tenant erasure |
| Mailbox `sms_verifications` | Codes | 24 hours, or 1 hour after release; at deposit to the vault | Maintenance alarm |
| Mailbox `calls` | Counterparty numbers, times, durations | As SMS | As SMS |
| D1 `phone_consent` | Keyed hash and masked hint | While the tenant exists; opt-outs survive counterparty erasure as hashes | Tenant erasure |
| D1 `phone_compliance_profiles`, `phone_compliance_documents`, R2 `t/{ten}/phone/docs/` | Company data, representative's identity data, identity documents | [§4.6](#46-retention-and-erasure) | Crypto-shred and delete; receipt |
| D1 `phone_numbers` | The tenant's own numbers (business data) | Life of the identity | Identity deletion, tenant erasure |
| D1 `phone_onboarding_drafts` | Business descriptions, sample messages and URLs an agent drafted; no personal data by schema | Until its request is created, or 30 days untouched | Deleted then; tenant erasure |
| D1 `phone_compliance_profiles` of a `sole_trader` | The proprietor's name, address, mobile (sealed) and tax data (sealed) | As any profile ([§4.6](#46-retention-and-erasure)) | As any profile |
| D1 `phone_requests` | Port-in losing-carrier secrets (sealed) | Cleared when the port ends; the row for 400 days after it ends | Retention job; tenant erasure |
| D1 `phone_provider_events`, `phone_message_index` | Provider IDs, no content | 30 days | Retention job |
| D1 `phone_charges` | None beyond IDs | 400 days after invoicing | Retention job; tenant erasure keeps only aggregate lines on the Stripe invoice |
| Mailbox `sms_messages` withheld as `vault_code` | Code-bearing SMS | 24 hours | Maintenance alarm; every erasure scope |
| Vault managed secret | Port-out PINs | Life of the number | `delete_managed` at release and port-out; the vault's erasure |

**Erasure scopes** ([Privacy § 6](privacy.md#6-erasure)) extend as follows:

- **SMS and call** (`DELETE …/sms/{sms_id}`, `DELETE …/calls/{call_id}`, `erasure:manage`): the row, its
  media and verification rows.
- **Counterparty**: a counterparty may now be an E.164 number; every SMS, media item and call with it in
  every identity of the tenant is deleted; its consent entry keeps the hash only.
- **Identity**: the mailbox wipe covers the new tables; numbers are released first ([§7.3](#73-identity-and-tenant-states)).
- **Tenant**: a new step `phone_release`, right after routing stops: cancel open requests, release every live
  number (the erasure request is the person's decision, because tenant erasure needs `tenants:erase`, which
  only the workspace owner's keys hold), cancel porting orders that can be cancelled, deactivate 10DLC
  campaigns (stopping their monthly fees), delete the tenant's provider resources and connectors, then the
  compliance data ([§4.6](#46-retention-and-erasure)).

**Receipt fields** added to [Privacy § 10](privacy.md#10-receipt): `sms_deleted`, `sms_media_deleted`,
`calls_deleted`, `phone_numbers_released` (with, for platform and partner keys, the handle
`{provider_number_id, provider_order_id, e164, vapi_phone_number_id}` of each, the shape Pylota's erasure
manifest uses), `phone_documents_deleted`, `phone_documents_retained` (with `retain_until`),
`phone_consent_entries_deleted`, and `provider_deletions` (`attempted`, `failed`).

**Processor side** ([Privacy § 11](privacy.md#11-what-remains-after-deletion) gains rows): Telnyx keeps message
and call detail records (numbers, times, costs) under its own retention, outside the Worker's reach;
message content is not kept at Telnyx when the profile's redaction is on (verified by
`live::phone::redaction_levels`); KYC documents and addresses are deleted at the provider after the last
number ([§4.6](#46-retention-and-erasure)); Vapi keeps the call data of `vapi` routes under the tenant's own
Vapi account. Erasure requests from a tenant's own customers reach these processors through the tenant
(for Vapi) and through the documented provider settings (for Telnyx), so text held at the provider is part of
every erasure plan.

**Subject-access export** includes `sms.json` and `calls.json` (the identity's, or a counterparty's
across identities) in the export ZIP.

**Jurisdiction.** D1, R2 and Durable Objects follow `PM_JURISDICTION` as for mail. Telnyx processes in the
US unless the regional endpoint `api.telnyx.eu` is used, and stores records where the account's data
locality says; processing location is "not contractually guaranteed" (Telnyx data locality documentation,
read 2026-10-24). Data transfer terms with Telnyx and Vapi are a question for counsel
([§32](#32-questions-for-counsel)).

## 25. Events and audit

**Mailbox events** (with the identity's `sequence`, [Webhook events](../../reference/events.md)):

| Type | When | `data` |
|---|---|---|
| `sms.received` | An inbound SMS is stored and visible | `sms` (summary with `text`, codes redacted), `conversation_id`, `trust` (`sender_unverified: true`, screening), `media[]` (id, content type, size) |
| `sms.quarantined` | An inbound SMS is stored but held | As `sms.received` without `text`, plus `quarantine_reason` |
| `sms.released` | A held SMS was released | `sms_id`, `released_by_key_id` or `released_by_user_id` |
| `sms.screened` | A screening verdict on an SMS | `sms_id`, `status`, `screening` |
| `sms.verification_received` | A code was found in a visible SMS | `sms_id`, `number_id`, `sender_hint`, `kind`, `account_id`; never the value |
| `sms.sent` | The provider accepted the SMS | `sms_id`, `segments`, `sender_used`, `sender_substituted` |
| `sms.delivered` | The handset or carrier confirmed it | `sms_id` |
| `sms.failed` | It was refused or not delivered | `sms_id`, `reason` |
| `sms.delivery_unconfirmed` | The carrier gave no final answer | `sms_id` |
| `sms.uncertain` | The outcome is unknown; never resent automatically | `sms_id`, `reason`, `fix` |
| `sms.reconciled` | An uncertain SMS was matched | `sms_id`, `status` |
| `call.started`, `call.ended` | A call on one of the identity's numbers | `call` (direction, number, masked counterparty, route kind, times, duration, outcome, `cost_pence`) |

An SMS withheld as `vault_code` emits no event at all ([§11.4](#114-codes-for-the-vault)).

**Platform events** (written with the D1 change, `sequence` null, `tenant_id` and `identity_id` set): 
`phone_request.requested` (also with `reason: "quote_changed"`), `phone_request.approved`,
`phone_request.rejected`, `phone_request.canceled`, `phone_request.completed`, `phone_request.failed`,
`phone_request.uncertain`; `phone_number.status_changed` (`from`, `to`, `setup_state`, `provisioning`,
`reason`); `phone_number.action_needed`; `phone_number.port_out_requested`, `phone_number.port_out_decided`;
`phone_registration.status_changed`; `phone_compliance.status_changed` (no personal data);
`phone_consent.changed` (masked hint); `phone_spend.warning` (`scope`, `percent`, `used_pence`, `cap_pence`);
`phone_number.capability_changed` (`number_id`, `capability`, `from`, `to`, `reason`), emitted when a
capability facet changes state, so an agent waiting for a US number to text learns of it without polling;
`phone_number.expiring` (`number_id`, `release_at`); `phone_setup.status_changed` (`country`, `from`, `to`,
`next_step`); `phone_budget.changed` (`version`, `enabled`, the decider's kind; no prices beyond the
ceilings).

Event feed visibility ([Event feed](event-feed.md)): SMS and call events need `phone:read`, held SMS events
also `quarantine:review`; request, number and registration events need `phone:read`; compliance events need
`phone:compliance`. Audit rows: `phone_request.*`, `phone_number.{move, adopt, primary}`,
`phone_route.set`, `phone_connector.{create, rotate, delete}`, `phone_compliance.{update, document_upload,
document_delete, export}`, `phone_budget.set`, `phone_setup.draft`, `phone_registration.otp`, `phone_port_out.{authorize, reject}`, `phone_consent.{opt_in, opt_out}`,
`phone_request.resolve` (platform).

## 26. Limits

| Limit | Value |
|---|---|
| Numbers per identity, per tenant | 5 and 20 by default (policy); at most 20 and 200 |
| Requests per tenant | 20 `pending_approval` (blocked ones included); 10 running (`executing`, `in_progress`, `action_needed`, `uncertain`) |
| Blocked request lifetime | 7 days after it is unblocked, at most 30 days after creation |
| Standing budget | `max_numbers_per_identity` 1–20; `max_new_numbers_per_month` 1–200 |
| One-call purchase | Inline run up to 15 seconds; up to 5 candidates when a number is taken; `details.alternatives` up to 5 |
| `expires_at` | 1 hour to 365 days after the purchase |
| Onboarding drafts | One per tenant and country; 2–5 sample messages of 20–1,024 characters; other text fields 1–1,000 characters; deleted after 30 days untouched |
| Sole-proprietor one-time code | 5 resends a day |
| Pending request lifetime | 7 days, then `rejected` (`expired`) |
| Quote token | 30 minutes |
| Searches | 300 a day per tenant; `limit` 1–20 per search |
| SMS text | 1–10 segments (GSM-7 153 characters a segment when split, UCS-2 67) |
| SMS caps | 200 segments a day per identity, 2,000 per tenant (policy) |
| Inbound SMS | 30 an hour per sender per identity, 600 an hour per identity (policy); text kept up to 10,000 characters |
| MMS media | 10 items, 5 MB each |
| `sms/wait` timeout | 1–60 seconds |
| Verification values | Released for 24 hours after arrival; repeatable for 1 hour after first release; vault deposits at most 15 minutes |
| KYC documents | PDF, PNG or JPEG; 1–20,971,520 bytes; 50 per tenant; upload URL one hour |
| Profile addresses | 20 |
| `sms_senders` per ledger entry | 10 |
| Voice connectors per tenant | 5 |
| Outbound call length | `phone.calls.max_minutes`, default 15, at most 120 |
| Port-out response | `respond_by` 24 hours after the request |
| Tenant reconcile | Once every 10 minutes |
| Webhook body | 256 KB |

## 27. Console and notifications

Console pages, server-rendered and working without JavaScript ([Console](console.md)):

- **Phone › Setup**: one card per country with the status of [§4.7](#47-phone-onboarding), the steps, the
  agent's draft beside the profile, the fees and waits; submitting creates and approves the request in one
  step after re-authentication; the sole-proprietor code entry.
- **Phone › Budget**: the standing budget, this month's use, and its history of versions; saving a looser
  budget needs a sign-in within the last 10 minutes.
- **Phone › Numbers**: per identity and per tenant; status, `setup_state`, the four capability facets with
  their next steps, route, `expires_at`, deadlines, action needed; move and primary (owners and admins).
- **Phone › Requests**: the tenant's queue with each quote, digest and payload summary (untrusted text
  escaped); approve and reject require a sign-in within the last 10 minutes, as policy saves do
  ([Workspace policy § 6](workspace-policy.md#6-the-console-page)).
- **Phone › Compliance**: the profile form, document uploads (write-only), status per country and type,
  expiry dates, provider comments.
- **Phone › Port-outs**: open port-outs first, with the time left; authorise and reject.
- **Phone › Charges**: this month's charges, caps and holds; past statements.
- **Overview › Needs a person** lists pending phone requests, port-outs, `requirements_pending` numbers,
  compliance actions and `uncertain` requests.

Account emails ([Notifications](notifications.md)): `phone_port_out` (urgent, to owners, admins and the
identity's accountable human; not subject to the daily cap), `phone_action_needed`,
`phone_requirements_deadline` (72 and 24 hours), `phone_compliance_expiring` (30 and 7 days),
`phone_compliance_legacy`, `phone_spend` (80% and 100%), `phone_request_uncertain` (after 24 hours),
`phone_setup_action_needed` (a person must act on onboarding, at most once a day per country),
`phone_number_ready` (a number became able to do everything its row promises, for example a US number once
its registration is ready). They
carry no SMS text and no counterparty number.

## 28. MCP and CLI

**REST routes** (the full contract is in [`openapi.yaml`](../../reference/openapi.yaml) and
[REST API › Phone numbers](../../reference/api.md)):

| Route | Permission |
|---|---|
| `GET /v1/phone/countries` | `phone:read` |
| `GET /v1/tenants/{tenant_id}/phone-setup`, `GET …/phone-setup/{country}` | `phone:read` |
| `PUT`, `GET`, `DELETE /v1/tenants/{tenant_id}/phone-setup/{country}/draft` | `phone:request` |
| `GET /v1/tenants/{tenant_id}/phone-budget` | `phone:read` |
| `PUT /v1/tenants/{tenant_id}/phone-budget` | `phone:manage` to tighten; loosening is a decision reserved for people |
| `POST /v1/identities/{identity_id}/phone-numbers` (one-call purchase) | `phone:request` |
| `POST /v1/tenants/{tenant_id}/phone-registrations/{registration_id}/otp`, `…/otp/resend` | `phone:compliance` (a person) |
| `GET /v1/identities/{identity_id}/phone-numbers/available` | `phone:request` |
| `POST /v1/identities/{identity_id}/phone-numbers/portability` | `phone:request` |
| `GET /v1/identities/{identity_id}/phone-numbers`, `GET …/phone-numbers/{number_id}` | `phone:read` |
| `PATCH …/phone-numbers/{number_id}`, `POST …/phone-numbers/{number_id}/move` | `phone:manage` |
| `GET …/phone-numbers/{number_id}/voice-route` | `phone:read` |
| `GET /v1/tenants/{tenant_id}/phone-numbers`, `GET /v1/phone-numbers/lookup` | `phone:read` (tenant level and above) |
| `POST /v1/identities/{identity_id}/phone-requests`, `GET` (list), `GET …/{request_id}` | `phone:request` (create, list, read); `phone:read` (read) |
| `GET /v1/tenants/{tenant_id}/phone-requests` | `phone:read` (tenant level and above) |
| `POST …/phone-requests/{request_id}/approve`, `…/reject` | `phone:approve` |
| `POST …/phone-requests/{request_id}/cancel` | `phone:request` (own) or `phone:approve` |
| `POST /v1/platform/phone-requests/{request_id}/resolve` | `platform:ops` |
| `GET`, `PATCH /v1/tenants/{tenant_id}/phone-compliance` | `phone:compliance` |
| `POST`, `GET /v1/tenants/{tenant_id}/phone-compliance/documents`, `GET`, `DELETE …/documents/{document_id}` | `phone:compliance` |
| `POST /v1/platform/phone-compliance/{profile_id}/export` | `platform:ops` |
| `GET /v1/tenants/{tenant_id}/phone-registrations` | `phone:read` |
| `POST`, `GET /v1/tenants/{tenant_id}/phone-connectors`, `DELETE …/{connector_id}`, `POST …/{connector_id}/rotate` | `phone:manage` |
| `POST /v1/identities/{identity_id}/sms`, `POST …/sms/{sms_id}/resolve` | `phone:send` |
| `GET /v1/identities/{identity_id}/sms`, `GET …/sms/{sms_id}`, `GET …/sms/{sms_id}/media/{n}`, `GET …/sms-conversations` | `phone:read` |
| `GET /v1/identities/{identity_id}/sms/wait` | `phone:read` |
| `POST …/sms/{sms_id}/release` | `quarantine:review` |
| `DELETE …/sms/{sms_id}`, `DELETE …/calls/{call_id}` | `erasure:manage` |
| `POST /v1/identities/{identity_id}/calls` | `phone:call` |
| `GET /v1/identities/{identity_id}/calls`, `GET …/calls/{call_id}`, `GET …/phone-usage` | `phone:read` |
| `GET /v1/tenants/{tenant_id}/phone-consent?number=`, `POST /v1/tenants/{tenant_id}/phone-consent` | `phone:read`; `phone:send` (opt-out) or `phone:manage` (opt-in) |
| `GET …/phone-numbers/{number_id}/port-outs`, `POST …/port-outs/{port_out_id}/authorize`, `…/reject` | `phone:read`; `phone:approve` |
| `GET /v1/tenants/{tenant_id}/phone-charges`, `GET /v1/partners/{partner_id}/phone-statement` | `phone:read` |
| `POST /v1/tenants/{tenant_id}/phone/reconcile`, `POST /v1/platform/phone/reconcile` | `phone:manage`; `platform:ops` |
| `POST /v1/platform/phone-numbers/adopt` | `platform:ops` |
| `POST /v1/platform/phone/probe` (the account-level probe of [§2.1](#21-one-account-and-provider_account_id-everywhere), run by `idvy setup phone` and `idvy doctor`) | `platform:ops` |
| `POST /hooks/telnyx` | no key; Ed25519 signature |

**MCP tools** (twelve, filtered by permission like the mail tools; [MCP server](mcp.md)): `phone_list_numbers`
(`phone:read`), `phone_search_numbers` (`phone:request`, read-only annotation), `phone_buy_number`
(`phone:request`; the one-call purchase of [§6.7](#67-one-call-purchase): `country` required, `type`,
`area_code`, `number`, `expires_at` optional, `idempotency_key` required; returns the number, or the request
with who must act and why. It has **no** `confirm` argument: the standing budget or a person decides on the
server, never a flag the agent sets), `phone_setup_status` (`phone:read`; the setup status of
[§4.7](#47-phone-onboarding) for one country or all, read-only), `phone_get_request` (`phone:request`),
`phone_send_sms` (`phone:send`; `idempotency_key` required), `phone_list_sms` and `phone_get_sms`
(`phone:read`), `phone_wait_sms` (`phone:read`; never returns a code claimed by a vault item),
`phone_place_call` (`phone:call`; returns the request when approval is required), `phone_list_calls`
(`phone:read`), `phone_record_opt_out` (`phone:send`). There is no tool to approve, release, port, submit
KYC, change a route, set a budget or reveal a PIN.

**CLI** (`idvy phone …`, [CLI](cli.md)): `countries`, `setup status|draft`, `budget get|set`, `buy`, `search`, `request number|port-in|release|route|emergency|a2p|toll-free|compliance`,
`requests list|get|approve|reject|cancel|resolve`, `numbers list|get|update|move|lookup|adopt`, `route get`,
`connectors create|list|rotate|delete`, `compliance get|update`, `documents upload|list|delete`, `export`,
`sms send|list|get|wait|resolve|release`, `calls create|list|get`, `consent get|set`, `port-outs list|authorize|reject`,
`registrations`, `usage`, `charges`, `statement`, `reconcile`; plus `idvy setup phone`. `sms send` and
`calls create` need an idempotency key, derived and reused as `send` does; `documents upload` computes the
size and SHA-256 locally and `PUT`s the file; `requests approve` without `--digest` reads the request,
prints the quote and asks for confirmation (non-interactive use must pass `--digest`).

## 29. Observability

Metrics ([Observability](observability.md)): `phone_webhook_total{type,result}`, `phone_webhook_ack_ms`,
`phone_event_mismatch_total`, `phone_request_total{kind,status}`, `phone_order_attempt_total{outcome}`,
`phone_order_uncertain_total`, `phone_numbers_live{status}`, `phone_facet_failures_total{facet}`,
`sms_send_total{result}`, `sms_dispatch_ms`, `sms_receipt_total{status}`, `sms_sender_substituted_total`,
`sms_inbound_total{status}`, `sms_wait_total{result}`, `sms_vault_code_total{result}`, `phone_spend_pence_total{kind}`,
`phone_spend_cap_denied_total{scope}`, `phone_charge_drift_pence`, `phone_provider_balance_usd`,
`phone_provider_errors_total{code}`, `phone_reconcile_drift_total{step}`, `phone_webhook_missed_total`,
`phone_port_out_open`, `call_total{direction,route,outcome}`, `phone_buy_total{result}` (`created`,
`pending`, `blocked`, `refused`), `phone_buy_inline_ms`, `phone_budget_covered_total{result}` (`covered`,
`outside`), `phone_setup_status{country,status}` (tenant counts), `phone_capability_wait_seconds{capability}`
(purchase to `enabled`), `phone_expiring_release_total{result}`. Labels never hold numbers, tenant names or
text.

Alerts: page `phone_port_out_pending`, `phone_port_out_deadline` (75% and 90%), `phone_provider_balance_low`,
`phone_provider_auth`, `phone_unplaced_outbound_call`, `phone_event_mismatch`; state
`phone_order_uncertain_stuck`, `phone_reconcile_drift`, `phone_orphan_number`, `phone_status_unknown`,
`phone_requirement_unmapped`, `phone_review_long`, `phone_webhook_signature_failures` (more than 10 in an
hour), `phone_fx_drift`, `phone_provider_limit_hit`, `phone_pin_missing`, `phone_recording_undeclared`,
`phone_release_unattributable`, `phone_order_duplicate`, `phone_registration_slow` (a US registration in
review for more than 21 days), `phone_expiring_release_stuck` (a scheduled release not done 24 hours after
`release_at`).

## 30. Rollout, telemetry and rollback

**The standing budget** is off on every tenant until a person sets it; on Pylota Identity Cloud the owner
turns it on for Pylota's operators first, with Pylota's own budgets, and watches `phone_budget_covered_total`
and the purchases it covered for seven days before offering it to others. `enabled: false` on a tenant, or the
ops switch at `paused_purchases`, stops covered purchases at once.

**Flags.** `PM_PHONE` (`off` by default; `on` enables the routes, the hook and the consumer). With it `off`,
every phone route answers `404 route_not_found`, `/hooks/telnyx` answers `503`, and no cron step runs. Per
tenant, `phone.allowed` gates use; on Pylota Identity Cloud the owner turns it on for Pylota's operators first
(platform or partner key), then for others.

**The ops switch `phone`** (`ops_switches`, set with `idvy ops switch phone …`): `on`; `paused_purchases`
(orders, ports, A2P fees, emergency registrations and voice-route changes refused with `503 unavailable`,
`details.reason = "phone_paused"`; SMS, calls, inbound, release and port-out decisions continue); `paused`
(also outbound SMS and calls refused; inbound, release and port-out decisions continue). It never releases a
number. It moves to `paused_purchases` automatically on `phone_provider_auth` and low balance
([§18.4](#184-provider-side-backstops-and-balance)).

**Telemetry before widening.** Each stage (Pylota's first operator, all Pylota operators, the waitlist) waits
for seven days with: no `phone_event_mismatch`; `phone_order_uncertain_stuck` zero; `phone_charge_drift_pence`
under 1% of spend; `phone_webhook_missed_total` zero after reconciliation; webhook acknowledgement p99 under
500 ms; no undecided port-out past 50% of its window.

**Rollback.** A code rollback is safe because the D1 tables are additive and numbers live at the provider:
turning `PM_PHONE` off stops new work and keeps every number, charge and record; turning it on again runs the
reconciler before anything else, which re-reads every number, order and port-out and repairs drift. Numbers
are never released by a rollback, a flag or a switch, only by an approved request or a tenant erasure.
A schema rollback is not needed: no existing column changes meaning (the one added column on
`service_accounts` and on the mailbox `idempotency` table are nullable).

## 31. Pylota agentic OS: parity and migration

**Parity** with Pylota's own integration (reviewed privately on 2026-10-24), item by item:

| # | Pylota today | Pylota Identity |
|---|---|---|
| 1 | GB mobile and US local, voice and SMS verified on the number; refusal for GB non-mobile SMS; region from the tenant's market | Catalogue rows GB mobile and US local with both capabilities required in search ([§3.2](#32-the-launch-catalogue), [§6.1](#61-search)); `422 country_not_offered` and capability errors are legible; the request names the country, chosen by Pylota from the operator's market (there is no default country) |
| 2 | At least one number per tenant; E.164 → tenant on active numbers, for SMS and voice webhooks; E.164 unique | Numbers per identity with tenant limits; the routing directory and `GET /v1/phone-numbers/lookup` match live numbers only; a unique live index ([§7.1](#71-the-directory-and-history)) |
| 3 | Three readiness facets read back; order status, requirements status, deadline; `provisioned`, `setup_state`, `provisioning{status,code,retryable}`; the code taxonomy and terminal set; no provider IDs to operators | All kept, the code set unchanged ([§6.4](#64-readiness-capabilities-and-pylotas-failure-codes)); provider IDs only for platform and partner keys |
| 4 | Lease shared with erasure; deletion-pending refusal; fail-closed preflight before spend; spend intent; `customer_reference` recovery and duplicate refusal; no re-order on uncertainty; no-spend mock mode; missing key is a configuration error | The execution lease and the erasure step's wait; claims refuse erasing tenants and deleting identities; the preflight of [§5.5](#55-execution-leases-and-attempts); spend intent with attempt-scoped references; reconciliation before any new attempt; test tenants and the simulator as the hard no-spend boundary; `not_configured` |
| 5 | Requirement-group discovery and per-country override; per-number attachment; porting groups refused; reviewable statuses non-terminal; the admin bundle flow | Per-tenant profiles and groups per (country, type, action) replace the platform group ([§4](#4-compliance-profiles-and-kyc-documents)); `porting` groups are used only by port-ins; review is non-terminal; the platform-wide group survives only for adopted numbers for 90 days ([§7.6](#76-adoption-for-pylotas-migration)) |
| 6 | Reconcile worker; read-repair with backoff; order webhooks by `customer_reference`; idempotent and lease-safe | The four movers of [§6.6](#66-who-moves-an-order) |
| 7 | Vapi import with a stored credential, `server.url` and `X-Vapi-Secret`, adopt by number, PATCH-then-GET, `vapi_phone_number_id`; outbound voice profile by hand | The `vapi` route ([§14.2](#142-vapi)); the outbound voice profile is created per tenant ([§8](#8-tenant-provider-resources)); approved outbound calls placed through Vapi ([§14.5](#145-outbound-calls)) |
| 8 | Inbound SMS contract: event ID, `from`, `to[0]`, text, MMS media, event types, signed, at least once with redelivery, per-sender rate limit, active-number lookup | `sms.received` and the receipt events, signed with Standard Webhooks, at least once, with the `evt_` ID for Pylota's replay keys ([Webhooks](webhooks.md)); per-sender limits; live-number routing. Pylota's handler keys idempotency on the Identity event ID instead of the Telnyx event ID |
| 9 | `send(from,to,text)` with four codes; sandbox; `external_id` equals the receipt ID; no silent resend; do-not-contact, monthly cap, 10DLC gates | The send route and its adapter mapping ([§12.1](#121-the-send-route)); test tenants; the `sms_` ID in every receipt; claim-then-send built in; the consent ledger, `phone.sms.tenant_monthly_cap` (fail closed) and the A2P gate. Pylota's strict cold-text decision stays in Pylota |
| 10 | 10DLC: per-tenant brand with sealed EIN, campaign, assignment; status view; enforce switch; owner-only submit | [§13](#13-us-a2p-10dlc-and-toll-free-verification); submission is a `phone:compliance` request a person approves; the gate cannot be switched off |
| 11 | Release: route then number, 404 success, discovery by IDs then reference then number, wait on pending, terminal orders on an empty listing, refuse unattributable, never trust a truncated listing, the erasure manifest handle | [§7.4](#74-release) and the receipt handle of [§24](#24-privacy) |
| 12 | Usage this month matching the cap; low-balance alerts | `GET …/phone-usage` ([§14.7](#147-call-records-and-cost)); balance alerts and the automatic pause ([§18.4](#184-provider-side-backstops-and-balance)) |
| 13 | Platform OTP sender for Pylota's own auth fallback | A number on an identity of Pylota's own platform tenant, sending with `kind: "verification"` ([§12.1](#121-the-send-route)) |

**Lessons** from the same review, each built into this page:

- US A2P registration is read from the provider's vetting fields, every fee step is approved, and one gate
  covers every send ([§13](#13-us-a2p-10dlc-and-toll-free-verification)).
- An order never leaves a tenant without a way forward: the explicit state machine, attempt-scoped
  references, deadline actions, provider-driven review and notification settings created at set-up
  ([§6.2](#62-the-provisioning-state-machine), [§6.3](#63-spend-intent-customer_reference-and-uncertain),
  [§6.5](#65-requirements-deadlines-and-long-reviews), [§2.4](#24-deployment-set-up)).
- Tenants share neither opt-out state nor regulatory identity: one messaging profile and one compliance
  profile per tenant, an ownership stamp on every number, senders bound to the identity, and signatures
  verified in every environment ([§8](#8-tenant-provider-resources), [§4](#4-compliance-profiles-and-kyc-documents),
  [§7.4](#74-release), [§12.1](#121-the-send-route), [§9.2](#92-verification)).
- One send path, caps that fail closed, receipts held for their target, and several numbers per identity
  ([§12](#12-outbound-sms)).

**What Pylota changes when it moves:** it calls the request, send, call and lookup routes instead of Telnyx
and Vapi; it stops sending its own STOP confirmations; its webhook handler takes Standard Webhooks events
from Pylota Identity (signature checked with the endpoint secret) instead of Telnyx and keeps its replay
lease keyed on the `evt_` ID; its approvals of numbers and calls call `…/approve` with its partner key and
`on_behalf_of`; and the owner retires the Telnyx key that Pylota's Vapi credential was created with, and
Pylota's own `TELNYX_*` environment, once every number is adopted or ported (row P107 of the register is
Pylota's own test of that cut-over).

## 32. Questions for counsel

Each is a legal question the design does not decide. The design's default is the cautious one, named after
each question.

1. **Regulated provider status.** Does TREFT LTD, by supplying UK numbers with outbound calls and SMS to
   business tenants through Telnyx, become a provider of a public electronic communications service subject
   to Ofcom's General Conditions (including registration duties), or is it a reseller of Telnyx's regulated
   service? The same question for ComReg (Ireland), ACM (Netherlands), BNetzA, ARCEP, CNMC and AGCOM. Telnyx's
   UK terms flow General Conditions B1 (numbering) and C6 (calling line identification) and the Good Practice
   Guide down to customers and end users (Telnyx country-specific terms, read 2026-10-24). Default: launch in
   GB only after an answer; numbers registered in the tenant's name.
2. **Emergency calling.** What does TREFT owe for numbers that can make outbound calls (UK 999 and 112, EU
   112, US 911 with E911 addresses)? Is "not supported, with a clear prior warning" lawful in each country?
   Who bears the Telnyx $100 penalty for an unregistered US emergency call? Default: emergency numbers are
   never dialled by agent routes; a warning before purchase; US numbers need a registered address before
   outbound calls ([§14.8](#148-emergency-calling)).
3. **Caller ID (Ofcom).** Do calls placed through Vapi's infrastructure, possibly outside the UK, presenting a
   UK number meet Ofcom's CLI rules and the 2025 measures on calls from abroad presenting UK numbers (Ofcom
   pages on blocking scam calls from abroad and the 2025 consultation, read 2026-10-24)? Default: a number
   presents only its own number as CLI; no spoofing.
4. **AI disclosure and consent to automated calls and texts.** Are the default disclosures sufficient under
   EU AI Act Article 50, the UK's position, PECR's rules on automated calls (specific prior consent for
   automated marketing calls: ICO guide to PECR, telephone marketing, read 2026-10-24) and the TCPA after FCC
   24-17? May a tenant turn them off? Default: on, and off only by a person.
5. **Recording consent** per jurisdiction (UK, Ireland, each EU state, US two-party-consent states).
   Default: recording off; with an announcement when on.
6. **KYC records.** The retention period per country for end-user identity records ("locally mandated"),
   the lawful basis, whether documents may be kept after a tenant's erasure request and for how long, the
   controller and processor roles of TREFT, the tenant and Telnyx, and the transfer terms for documents sent
   to Telnyx (US) and Vapi. Default: `PM_PHONE_KYC_RETENTION_DAYS` = 365 after the last number, retained
   documents listed in the receipt ([§4.6](#46-retention-and-erasure)).
7. **Sub-allocation.** Are numbers registered in the tenant's own name but held in TREFT's provider account
   "sub-allocated" under French, Italian or UK rules (Telnyx's UK form asks whether the customer is
   sub-allocating)? What do the reseller annex's indemnity and lawful-interception clauses mean for TREFT?
   Default: France and Italy only for tenants with physical presence, numbers in their own name.
8. **Marketing SMS.** Is the consent ledger's evidence enough under PECR (including the soft opt-in), the
   TCPA and carrier codes? Is STOP handling by the provider's auto-response adequate in the UK? Default:
   marketing needs a recorded opt-in with evidence.
9. **Port-outs.** On what grounds may a port-out be rejected in the UK and the EU, and may a PIN be required?
   Default: never rejected automatically; a person decides.
10. **Sole traders** (no longer blocking: the owner refused them on 2026-10-26, [§1.1](#11-sole-traders-refused-owner-decision-2026-10-26)).
    Is excluding sole traders consistent with the consumer and telecoms rules that apply to the tenants' own
    customers, and with the dated exit given to adopted numbers? A different answer would reopen options (a)
    and (b).
11. **US registrations by a UK ISV** (blocking for texting from US numbers by tenants registered outside the
    US). What does The Campaign Registry require of a UK ISV registering brands for US businesses, and of a UK
    business registering a US brand? Default: `brand_countries = ["US"]` until answered and
    `live::phone::uk_brand_registration` passes ([§13.2](#132-brands-registered-outside-the-us)).
12. **Standing budgets.** Does a purchase made under a budget a person set in advance satisfy the reseller
    terms and the tenant's own records as "the tenant's decision" in the same way as a purchase approved one
    by one? Default: yes, with the budget's version and decider recorded on every purchase it covers.
13. **Tax.** VAT on resold telecommunications services to UK and EU businesses (place of supply), and
    whether Stripe Tax covers it, is a question for the accountant as well as counsel.


## 33. Functional requirements

To add to [PRD § 6](../prd.md) as a new group, "Phone numbers". Priorities follow PRD § 5; the whole group is
behind `PM_PHONE`. "Acceptance" names the edge rows whose tests prove the requirement.

- **FR-PHN-1** (P0) Phone numbers **must** be off unless `PM_PHONE` is `on` and the tenant's `phone.allowed`
  is `true`; the ops switch `phone` **must** support `on`, `paused_purchases` and `paused` as specified, and
  no flag, switch or rollback **may** release a number. Acceptance: P4, P101.
- **FR-PHN-2** (P0) The service **must** reach the telephony provider only through the `PhoneProvider`
  trait, at fixed hosts, with one Telnyx account in v1 and `provider` and `provider_account_id` stored on
  every row that names a provider resource; no route, tool, event or log **may** expose a provider credential.
  Acceptance: P103.
- **FR-PHN-3** (P0) What can be bought **must** come from a versioned catalogue of rows (country, number type,
  status `available`, `requires_local_presence` or `not_offered`, capabilities, requirements, local presence,
  sender rules, quiet hours, emergency settings, sources and verification date); GB mobile, GB local and US
  local rows are P0, US toll-free, IE, NL and the DE, FR, ES and IT local rows P1. Acceptance: P1, P3.
- **FR-PHN-4** (P0) Number search **must** require every capability of the row on each candidate, never
  substitute another number type, drop candidates above the row's cost ceiling and numbers in the 90-day
  cross-tenant quarantine, return a GBP quote and a signed quote token per candidate, and count against a
  daily cap. Acceptance: P1, P2, P39.
- **FR-PHN-5** (P0) Each tenant **must** have one compliance profile describing a business in whose legal
  name its numbers are registered; individuals **must** be refused, and sole traders **must** be refused unless
  the owner's recorded decision and the row allow them (FR-PHN-54); tax identifiers and
  the representative's contact and birth data **must** be sealed and never returned. Acceptance: P5, P6.
- **FR-PHN-6** (P0) KYC documents **must** be uploaded with declared size and SHA-256, sniffed and refused when
  risky, encrypted per document under a sealed data key before they reach R2, and never readable through any
  route; deleting one **must** crypto-shred it. Acceptance: P7, P8, P9.
- **FR-PHN-7** (P0) Provider requirements **must** be read as data and mapped to profile fields and document
  kinds; submitting a profile for a country, type and action **must** be a request a person approves, whose
  run uploads and links documents within one run and validates addresses; provider group states **must** map
  to profile statuses, with expiry notifications. Acceptance: P10–P14.
- **FR-PHN-8** (P0) KYC records **must** be kept while needed and for `PM_PHONE_KYC_RETENTION_DAYS` after the
  last number, then deleted; tenant erasure **must** delete them except documents in their retention period,
  which the receipt lists; provider copies **must** be deleted after the last number; a platform export
  **must** be single-use and audited. Acceptance: P98, P99.
- **FR-PHN-9** (P0) Every purchase, port-in, release, compliance submission, US registration, toll-free
  verification, emergency registration, voice-route change and (under policy) outbound call **must** be a phone
  request (a purchase may be approved by the standing budget, FR-PHN-43, and a scheduled release by its
  purchase, FR-PHN-50), created with a required `Idempotency-Key`, bounded per tenant, and rejected as expired after 7
  days undecided. Acceptance: P15, P19, P22.
- **FR-PHN-10** (P0) Approving a phone request or deciding a port-out **must** be a decision reserved for
  people (`key_may_decide`, opt-in `phone.key_approval` for platform and own-partner keys only), never by the
  requesting key, bound to the request's digest, and audited with `on_behalf_of`. Acceptance: P16, P17, P18.
- **FR-PHN-11** (P0) Quotes **must** be in GBP from the rate card; a higher price at execution **must** return
  the request to `pending_approval` with a new digest; provider costs in USD **must** be shown only to platform
  and partner keys. Acceptance: P20, P76.
- **FR-PHN-12** (P0) A request **must** run under one claim with a fencing token, re-check the ops switch,
  configuration, policy, compliance, tenant resources and the spend hold before any billable call, and never
  start for an erasing tenant or a deleting identity. Acceptance: P21, P33, P37.
- **FR-PHN-13** (P0) Every order attempt **must** write a spend intent and an attempt-scoped
  `customer_reference` first; a definitive refusal **must** clear the intent only after a complete listing
  proves no order; an unknown outcome **must** become `uncertain` and **must not** be retried until an order is
  adopted or absence is proved by two complete listings at least 5 and 10 minutes apart; at most 3 attempts;
  after 24 hours only a platform resolve ends it. Acceptance: P23–P27.
- **FR-PHN-14** (P0) Numbers **must** follow the state machine of § 6.2, with carrier, SMS and voice facets each
  read back from the provider, the capability facets of FR-PHN-45, `setup_state` and `provisioning { status, code, retryable }` using Pylota's
  failure codes unchanged, and no provider IDs for tenant or identity keys. Acceptance: P34.
- **FR-PHN-15** (P0) Requirements pending, deadlines and long reviews **must** notify people, remind before
  deadlines, back off and alert after 7 days, and end a cancelled order as `failed` so the next request can
  order again. Acceptance: P28, P29, P30.
- **FR-PHN-16** (P0) Orders **must** be moved by provider events, the every-minute cron, read-repair that only
  queues work with a written backoff, and the nightly reconciler, all idempotent under the claim.
  Acceptance: P21, P96.
- **FR-PHN-17** (P0) A number **must** be an address of one identity: a unique live E.164 in the directory,
  history kept after release, port-out or move, routing and lookup on live numbers only, primary and alias
  roles per capability, moves within a tenant, and per-identity and per-tenant limits. Acceptance: P23, P40,
  P103.
- **FR-PHN-18** (P0) Deleting an identity that holds a live number **must** be refused; a paused identity
  **must** keep receiving and refuse sends; a suspended tenant **must** refuse sends and requests.
  Acceptance: P35, P36, P37.
- **FR-PHN-19** (P0) Release **must** be an approved request that discovers the number by stored IDs, then
  reference, then exact number in a complete listing, waits on pending orders, proves the ownership stamp,
  removes the voice route first and treats `404` as success; a released number **must not** be sold to
  another tenant for 90 days. Acceptance: P38, P39.
- **FR-PHN-20** (P0) Each tenant **must** have its own messaging profile, credential connection, outbound
  voice profile and billing group at the provider, found by name and tag before any create, read back after
  every change, and kept in step with policy. Acceptance: P41, P55.
- **FR-PHN-21** (P0) `POST /hooks/telnyx` **must** verify the Ed25519 signature over the timestamp and raw body
  with a 5-minute window in every environment, write the raw event to R2 and queue a pointer before answering
  `2xx`, read nothing from D1, deduplicate on the event ID and apply events in `occurred_at` order per
  resource. Acceptance: P42–P45, P47.
- **FR-PHN-22** (P0) The owner of every provider event **must** come from a fact the provider controls and be
  cross-checked with a second fact where one exists; a mismatch **must** have no effect and alert.
  Acceptance: P46, P48.
- **FR-PHN-23** (P0) Inbound SMS **must** be stored in the identity's mailbox with the trust flag
  `sender_unverified`, decided by the SMS quarantine rules, never refused for a plan or spend reason, and handled
  by the simulator in test tenants. Acceptance: P49, P52, P56.
- **FR-PHN-24** (P1) Inbound SMS **must** be screened as mail is (the deterministic scan, and in `hold` mode the
  classifier and deadline), with short and keyword messages exempt from the model. Acceptance: P51.
- **FR-PHN-25** (P0) A consent ledger per tenant brand **must** record opt-outs and opt-ins by keyed hash, apply
  STOP, START and HELP whatever the message's other status, never send Pylota Identity's own confirmation,
  accept opt-ins only with `phone:manage` and evidence, survive counterparty erasure, and stay in step with the
  provider's list. Acceptance: P50, P53, P55, P67, P105.
- **FR-PHN-26** (P0) Verification codes in SMS **must** be extracted without the link rule, and where
  `accounts.require_approval` is true reach an agent only through an approved service-ledger entry naming the
  number, a registered wait and, when listed, the sender. Acceptance: P57, P58, P62.
- **FR-PHN-27** (P0) Codes on a number named by an active vault login item's phone channel **must** be withheld
  as `vault_code`, never shown to agents or API keys, and given only to the vault's `SmsCodeSource` once;
  `vault.codes_into_fills_only` **must** withhold every code SMS; reads and events **must** redact codes; no log
  **may** hold SMS text or codes. Acceptance: P59, P60, P61.
- **FR-PHN-28** (P0) Outbound SMS **must** use one send route with a required `Idempotency-Key`, a sender bound to
  the sending identity, and the ordered pipeline of § 12.2, whose caps fail closed; a dry run **must** check
  without holding or storing. Acceptance: P64, P65, P69, P70, P71, P106.
- **FR-PHN-29** (P0) Dispatch **must** make one provider call per SMS under a transport claim, with the provider
  idempotency key `pi-sms-{sms_id}`; repeats with the same key **may** be enabled only after the contract test
  passes; an unknown outcome **must** end `uncertain`, never be resent automatically, and be reconciled by
  matching or resolved by the integrator. Acceptance: P63.
- **FR-PHN-30** (P0) Receipts **must** set SMS statuses, record the sender actually used and flag substitution,
  hold a receipt that arrives before its target for 15 minutes, and settle the spend hold with the provider's
  cost. Acceptance: P66, P72, P75.
- **FR-PHN-31** (P0 for 10DLC, P1 for toll-free) US A2P **must** use one brand and campaign per tenant,
  readiness only at `MNO_PROVISIONED`, brand approval at `VERIFIED` or `VETTED_VERIFIED`, `webhookURL` set,
  creation only inside an approved and claimed request with a lookup before paying again, and a send gate for
  every US sender; simulated IDs **must never** be stored for live tenants. Acceptance: P68, P81–P84.
- **FR-PHN-32** (P0 for `vapi`, P1 for `sip` and `forward`) Each voice-capable number **must** have one route
  (`vapi`, `sip` inbound, `forward` or `none`) set by an approved request and read back; the `vapi` route
  **must** use Vapi's bring-your-own SIP trunk with the tenant's SIP credentials, never a Telnyx key, adopt by
  exact number and verify by PATCH then GET. Acceptance: P85, P86, P89.
- **FR-PHN-33** (P0) Outbound calls **must** go through a `vapi` route only in v1, refuse emergency, short and
  premium destinations, follow `phone.calls.outbound`, speak the disclosure first, hold their maximum cost, and
  become `uncertain` rather than be placed twice; calls the service did not place **must** be detected.
  Acceptance: P87, P88, P90.
- **FR-PHN-34** (P0) Recording **must** be off by default and on only with an announcement; transcripts the
  service holds **must** be untrusted input; call records and usage **must** count what the caps count.
  Acceptance: P90.
- **FR-PHN-35** (P0) Each catalogue row **must** carry an emergency setting; US numbers **must** have a
  registered emergency address (an approved request) before their routes place calls; agent routes **must
  never** dial emergency numbers; tenants **must** be warned before purchase where emergency calls are not
  supported. Acceptance: P88, P91.
- **FR-PHN-36** (P1) Port-in **must** be an approved request with a free portability check, sealed losing-carrier
  details, provider splits as separate records, exceptions as `action_needed`, and no cancellation within 48
  hours of the FOC date. Acceptance: P92.
- **FR-PHN-37** (P0) Every number **must** carry a random port-out PIN kept only in the vault; a port-out
  **must** alert people at once, be decided only by a person, never be rejected automatically, escalate at 50%,
  75% and 90% of its window, and rotate the PIN after a rejection or reveal. Acceptance: P93, P94, P95, P102.
- **FR-PHN-38** (P0) The provider's state **must** be reconciled continuously and nightly across numbers,
  orders, porting, compliance, A2P, message and call records, webhook deliveries, opt-outs and balance, and on
  request per tenant and per deployment. Acceptance: P96, P97.
- **FR-PHN-39** (P0) Money **must** follow § 18: GBP prices from the rate card, one charge per unique source,
  monthly spend caps per tenant and identity enforced with holds and refused with `402 spend_cap_reached` before
  anything is stored, recurring charges never refused, provider-side daily limits as backstops, balance alerts
  with an automatic pause, Stripe invoice items with idempotency keys, partner statements, and a paid plan or a
  partner for self-serve tenants. Acceptance: P73–P80.
- **FR-PHN-40** (P0) Erasure scopes **must** cover SMS, media, calls and phone data with the receipt fields of
  § 24; tenant erasure **must** run `phone_release`; logs **must** pseudonymise phone numbers; every sealed column
  **must** be in the sealed-column registry. Acceptance: P53, P98, P61.
- **FR-PHN-41** (P0) The MCP server **must** offer the twelve `phone_*` tools of § 28 and no tool that approves,
  releases, ports, submits KYC, changes a route, sets a budget or shows a PIN; `phone_buy_number` **must not**
  take a confirmation flag; the CLI **must** offer `idvy phone …` and
  `idvy setup phone`. Acceptance: `it::mcp::phone_tools`, `cli::phone::commands`.
- **FR-PHN-42** (P0) A platform key **must** be able to adopt a number already in the provider account into an
  identity, stamping ownership and refusing a number already held; an adopted number registered under a
  platform group **must** be re-papered under its tenant's profile by its `legacy_deadline`, and **must never**
  be released by a deadline (FR-PHN-54). Acceptance: P100, P126.
- **FR-PHN-43** (P0) A tenant **may** have a standing purchase budget (`phone.auto_approve`), off by default,
  set or loosened only by a decision reserved for people and tightened by `phone:manage`; a `purchase_number`
  it covers **must** be created approved with the budget's version recorded, count against a monthly slot
  taken atomically with the spend hold, and remain subject to the spend caps, the ops switch and quote changes.
  Acceptance: P108–P111.
- **FR-PHN-44** (P0) `POST /v1/identities/{identity_id}/phone-numbers` **must** buy a number in one call with a
  required country and a required `Idempotency-Key`, defaulting the type per country, answering `201` with the
  number once the provider accepted the order inside the budget, or `202` with the request otherwise; a taken
  number **must** be replaced by the next candidate up to 5 times for `any`, and answered `409
  phone_number_taken` with up to 5 alternatives for an exact number. Acceptance: P112–P114.
- **FR-PHN-45** (P0) Every number **must** report the capability facets `outbound_sms`, `inbound_sms`,
  `inbound_calls` and `outbound_calls` as `enabled`, `activating`, `registration_required` or `not_available`
  with a reason and a next step, computed by the same function the send and call gates use, and emit
  `phone_number.capability_changed` on each change; `setup_state` **must** be `registration_pending` when only a
  registration is missing. Acceptance: P115, P116.
- **FR-PHN-46** (P0) A purchase quote **must** carry `follow_on[]` for every registration the number will still
  need for this tenant, with its price, typical wait and who acts, covered by the digest. Acceptance: P117.
- **FR-PHN-47** (P0) A purchase for a row the tenant has not finished onboarding **must** be created with
  `blocked_by` and a next step rather than refused, **must not** be executed while blocked, and **must** run
  once onboarding completes. Acceptance: P118.
- **FR-PHN-48** (P0) One-off registration fees **must** count against `phone.spend.registration_cap_pence`
  instead of the monthly caps; pass-through fees **must** be priced at cost, with no margin (owner decision, 2026-10-26).
  Acceptance: P119.
- **FR-PHN-49** (P0) Phone onboarding **must** report one status per tenant and country with its steps, fees,
  waits and next step through REST and the MCP tool `phone_setup_status`; agents **may** draft the registration
  text, which **must** hold no tax identifier, personal data or document and **must** be submitted only by a
  person. Acceptance: P120, P121.
- **FR-PHN-50** (P0) A purchase **may** carry `expires_at`; its approval **must** cover the release at that time,
  which **must** run with every release safety rule; extending or removing the expiry **must** need
  `phone:manage`. Acceptance: P122.
- **FR-PHN-51** (P0) The profile **must** name the company register (with its region where a country has
  several), and document requirements **must** be mapped per registration country, never guessed.
  Acceptance: P123.
- **FR-PHN-52** (P0) A US number of a tenant registered outside the US **must** report `outbound_sms:
  not_available` until GB brands are proved and allowed, and `outbound_calls: not_available` without a US
  emergency address; SMS **must** be refused to destinations the sending row has not tested. Acceptance: P124,
  P125.
- **FR-PHN-53** (P0) The provider account's level **must** be probed and recorded, and a row that needs a level
  the account lacks **must** report `platform_prerequisite`; US rows **must** stay off until the enablement
  sequence of § 2.5 is complete. Acceptance: P127.
- **FR-PHN-54** (P0) Sole traders **must** be refused (owner decision of 2026-10-26, ADR 0020 decision 15); an
  adopted number of a sole-trader tenant **must** keep working with a dated exit (re-paper or port out), no
  earlier than 2027-01-24, and be suspended for outbound only, never released, at its deadline. Acceptance: P6,
  P126.

## 34. Edge cases

Section `P · Phone numbers` of the [edge-case register](../edge-cases.md) (rows P1–P131; P107 is
integrator-owned):

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| P1 | Search on an offered row | Candidates carry every capability of the row (a GB mobile without `sms` is never returned), a GBP quote and a 30-minute quote token; only the requested number type | S | `core::phone::catalogue::rows_valid`, `it::phone::search_quotes_and_features` |
| P2 | A candidate whose provider cost exceeds the row's ceiling (vanity, premium) | Dropped from results; never orderable | S | `it::phone::search_drops_premium` |
| P3 | Search or request for a `not_offered` row, or a `requires_local_presence` row without the presence | `422 country_not_offered`; `422 local_presence_required` with `details.local_presence`; nothing stored | S | `core::phone::catalogue::eligibility`, `it::phone::catalogue_refusals` |
| P4 | `phone.allowed` false; a Free-plan tenant on a Stripe deployment; `PM_PHONE` off | `403 policy_denied` (`phone_not_allowed`, `plan_required`); with the flag off every phone route `404 route_not_found` | S | `it::phone::gates` |
| P5 | A purchase for a row needing an approved profile, without one; a port-in without one | The purchase is created with `blocked_by: ["compliance"]` and never executed while blocked; the port-in gets `409 compliance_not_approved`; no provider call | S | `it::phone::compliance_required` |
| P6 | A profile with an individual or a sole-trader entity type | `422 compliance_incomplete`, `details.reason = "entity_type"` | S | `core::phone::requests::profile_validate` |
| P7 | Document: wrong content type, over 20,971,520 bytes, checksum mismatch, executable or macro sniffed | `400 invalid_request`, `400 upload_size_mismatch`, `400 upload_checksum_mismatch`, `422 attachment_risky`; nothing stored | S | `it::phone::document_upload_rules` |
| P8 | Reading a document | Metadata only on every route; no route returns bytes; the R2 object is ciphertext under a per-document sealed key | S | `it::phone::documents_write_only` |
| P9 | Deleting a document a live provider group needs | `409 phone_document_in_use`; deletion of an unused one shreds the key, then deletes the object | S | `it::phone::document_delete` |
| P10 | A submission run that cannot link documents within 20 minutes of the first upload | Uploaded provider documents deleted once unlinked; the request `failed` (`upstream_error`); no document left unlinked at the provider | S | `it::phone::submission_link_window` |
| P11 | A provider requirement type with no mapping | `422 compliance_incomplete` with `details.missing[]`; `phone_requirement_unmapped` raised | S | `core::phone::catalogue::map_requirements`, `it::phone::requirement_unmapped` |
| P12 | Provider address validation fails | Request `failed` (`compliance_address_invalid`) with `details.suggested`; no address stored unvalidated for a number that can call | S | `it::phone::address_validation` |
| P13 | A requirement group declined | Profile row `rejected` with the comments as escaped untrusted text; owners notified; a new request resubmits | S | `it::phone::group_declined` |
| P14 | A group `expired`, `no-longer-eligible`, or an unknown status | `expired`; an unknown value keeps the status and raises `phone_status_unknown` | S | `core::phone::telnyx::group_status_table` |
| P15 | Request without `Idempotency-Key`; repeated with the same key and body; same key, other body | `400 idempotency_key_required`; the stored response with `Idempotent-Replayed: true`; `409 idempotency_conflict` | S | `it::phone::request_idempotency` |
| P16 | A tenant key approves where people decide; a partner key under `phone.key_approval`; a tenant key with `phone.key_approval` | `403 permission_denied` (`person_required`); allowed; still `person_required` | S | `it::phone::approval_who` |
| P17 | A key approves a request it created | `403 permission_denied` (`self_approval`) | S | `it::phone::approval_who` |
| P18 | Approval naming a stale digest; approval of a request not `pending_approval` | `409 phone_request_digest_mismatch` with `details.current_digest`; `409 phone_request_not_pending` | S | `it::phone::approval_binding` |
| P19 | A pending request past 7 days | `rejected` (`expired`) by the cron with its event; a late approval gets `409 phone_request_not_pending` | S | `it::phone::request_expiry` |
| P20 | A higher price at execution than quoted; a lower one | Back to `pending_approval` with a new quote and digest (`reason: "quote_changed"`), approval void; the lower price is charged | S | `it::phone::quote_changed` |
| P21 | Two approvals racing, a redelivered `execute`, a cron sweep, and a run whose lease lapsed | One run at a time; the stale run's guarded writes change nothing and it stops | S | `core::phone::requests::claim_rules`, `it::phone::claim_and_fencing` |
| P22 | The 21st pending request of a tenant, or the 11th running | `422 phone_request_limit_reached` | S | `it::phone::request_limits` |
| P23 | Two tenants order the same number at once | The second fails at the directory's unique live index before any provider call; no spend | S | `it::phone::same_number_two_tenants` |
| P24 | The number is taken between search and order (85001) | Listing proves no order; spend intent cleared; hold released; with `selection.mode = "any"` the run orders the next candidate (up to 5), else the number `failed` (`no_numbers_available`) and the request `failed` | S | `it::phone::number_taken` |
| P25 | A definitive refusal whose reference listing is incomplete or fails | The attempt is `uncertain`; nothing cleared by guess | S | `core::phone::orders::absence_proved`, `it::phone::refusal_needs_listing` |
| P26 | The order call times out and the order exists | `uncertain`; the reconciler finds it by reference and adopts it; exactly one order | S | `it::phone::uncertain_found` |
| P27 | The order call times out and no order exists; or nothing can be proved for 24 hours | Two complete empty listings (≥ 5 min, then ≥ 10 min) prove absence; the next attempt uses a new reference (at most 3); otherwise `carrier_order_uncertain`, `phone_order_uncertain_stuck`, and only the platform resolve ends it | S | `core::phone::orders::absence_proved`, `it::phone::uncertain_absent_and_stuck` |
| P28 | Requirements pending on an order | `requirements_pending`, `phone_number.action_needed`, notifications; reminders at 72 and 24 hours before `deadline_at` | S | `it::phone::requirements_pending` |
| P29 | The deadline passes and the provider cancels | Number `failed` (`regulatory_rejected`, `deadline_passed`) after a complete listing shows no live number; spend intent cleared; the next request orders | S | `it::phone::deadline_cancel_not_stranded` |
| P30 | A review lasting 7 days | Checks at 1 h, 6 h, then daily; `phone_review_long` at 7 days; no loop and no `provision_in_progress` | S | `it::phone::long_review_backoff` |
| P31 | Provider `402` or code 20100 on an order | `carrier_billing`; page alert; nothing ordered | S | `it::phone::provider_billing` |
| P32 | Provider `401` or `403` | `not_configured`; `phone_provider_auth`; the ops switch moves to `paused_purchases` | S | `it::phone::provider_auth` |
| P33 | `PM_TELNYX_API_KEY` missing | `not_configured` before any provider call; not retried | S | `it::phone::not_configured` |
| P34 | Carrier active but the messaging read-back differs | Retried; then `sms_status = failed` (`sms_configuration`); the number stays `configuring`; voice unaffected | S | `it::phone::facets_independent` |
| P35 | Deleting an identity that holds a live number | `409 identity_has_phone_numbers` | S | `it::phone::identity_delete_refused` |
| P36 | A paused identity | Inbound SMS stored and evented; sends `409 identity_paused`; requests execute | S | `it::phone::identity_paused` |
| P37 | The tenant is suspended during a run | The run's next guarded write fails and it stops; sends `403 tenant_suspended` | S | `it::phone::tenant_suspended_mid_run` |
| P38 | Release of a live number without the ownership stamp; a truncated listing; a pending order; a `404` | Refused with `phone_release_unattributable`; never read as absent; waits; success. The voice route is removed before the number | S | `it::phone::release_safety` |
| P39 | A number released by tenant A offered to tenant B within 90 days | Excluded from B's search and refused at execution; A may buy it back | S | `it::phone::release_quarantine` |
| P40 | Moving a number to another identity of the tenant, and to an identity of another tenant | Moved, history kept with the old identity; `404 identity_not_found` | S | `it::phone::move_number` |
| P41 | A crash after creating the tenant's messaging profile and before recording it | The next run finds it by name and tag and adopts it; no second profile | S | `it::phone::tenant_resources_adopted` |
| P42 | A webhook with a bad or missing signature, or a timestamp outside 5 minutes, in a test tenant or staging | `403 invalid_signature`; nothing staged | S | `core::phone::telnyx::verify_signature`, `it::phone::hook_signature` |
| P43 | `PM_TELNYX_PUBLIC_KEY` unset | `503 unavailable` (`not_configured`); nothing processed | S | `it::phone::hook_signature` |
| P44 | R2 or the queue fails in the hook; D1 is down | `503` (the provider retries); with D1 down the hook still answers `200` | S | `it::phone::hook_staging_failures` |
| P45 | The same event delivered twice | One effect | S | `it::phone::hook_dedup` |
| P46 | An event whose second fact disagrees (profile or connection of another tenant) | No effect; `ignored_mismatch`; `phone_event_mismatch` | S | `it::phone::event_cross_check` |
| P47 | A late `message.sent` after `delivered` | Ignored by `occurred_at` | S | `it::phone::event_ordering` |
| P48 | SMS to an unknown, released, ported-out or failed number | `ignored_unrouted`; nothing stored | S | `it::phone::sms_unrouted` |
| P49 | An inbound SMS | Stored with `sender_unverified`; text untrusted; never refused for plan or spend | S | `it::phone::sms_inbound_stored` |
| P50 | STOP, START or HELP from a blocked or throttled sender | The consent effect applies anyway; no confirmation from Pylota Identity | S | `it::phone::keywords_always_apply` |
| P51 | Screening in `hold`: decisive scan; undecided; short or keyword message; classifier unavailable with `on_unavailable = quarantine` | `quarantined` (`steering_suspected`); `screening` then a verdict; skips the model; `quarantined` | S | `it::phone::sms_screening` |
| P52 | A sender over 30 an hour, or an identity over 600 | `throttled`; not evented | S | `it::phone::sms_inbound_caps` |
| P53 | Counterparty erasure of an E.164 | Its SMS, media and calls deleted in every identity of the tenant; the opt-out kept as a hash; the next send refused | S | `it::phone::counterparty_erasure` |
| P54 | MMS with more than 10 items, an item over 5 MB, a host not on the list, a risky item | Items beyond the limit `unavailable`; foreign hosts never fetched; a risky item quarantines the SMS (`risky_attachment`) | S | `it::phone::mms_media` |
| P55 | A STOP to tenant A's number | Tenant B's sends to the same person are unaffected at both layers | S | `it::phone::stop_isolated` |
| P56 | SMS in a test tenant | Simulator only; no provider call; simulated IDs stay in the test tenant | S | `it::phone::test_tenant_simulator` |
| P57 | A solicited code from an approved entry's listed sender | Released by `sms/wait`; repeat within 1 hour returns the same value; then purged | S | `it::phone::sms_wait_release` |
| P58 | A code with `accounts.require_approval` and no matching entry; `sms/wait` without `account_id` | `quarantined` (`account_unapproved`); `403 policy_denied` (`account_not_approved`) | S | `it::phone::sms_ledger_gate` |
| P59 | A code SMS on a number named by an active vault login item's phone channel | `hidden` (`vault_code`); no events; withheld from keys' reads; taken once by `SmsCodeSource::take`; deleted after 24 hours | S | `it::phone::vault_code_withheld`, `it::vault::sms_otp_phase` |
| P60 | `vault.codes_into_fills_only` true | Every code SMS withheld except receive-allowed senders; `sms/wait` `kind=verification` gets `403 policy_denied` (`codes_into_fills_only`) | S | `it::phone::vault_strict_mode` |
| P61 | Reads, events and logs of a code-bearing SMS | Text shows `[code]`; the canary code appears in no log line | S | `it::phone::code_redaction` |
| P62 | An unsolicited code with `quarantine.unsolicited_otp`; a link in an SMS | `quarantined` (`otp_unsolicited`); a link is never a verification object | S | `core::phone::verify::find_code`, `it::phone::sms_unsolicited` |
| P63 | Unknown outcome on `POST /messages` | Repeats with the same provider key (only once the contract test passed) send one SMS; then `uncertain`, never resent | S | `live::phone::send_idempotency`, `it::phone::sms_unknown_outcome` |
| P64 | Send repeated with the same key and body; other body; in progress | Stored response; `409 idempotency_conflict`; `409 request_in_progress` | S | `it::phone::sms_idempotency` |
| P65 | `from` of another identity, a platform number, a released number; a number still configuring | `404 phone_number_not_found`; `409 phone_number_not_ready` | S | `it::phone::sms_sender_binding` |
| P66 | A receipt before the provider ID is stored | Held `pending_target` up to 15 minutes, then applied | S | `it::phone::receipt_race` |
| P67 | An opted-out recipient; marketing without opt-in; provider 40300 | `403 recipient_opted_out`; `403 consent_required`; ledger entry `provider_sync` | S | `it::phone::sms_consent` |
| P68 | A US sender with a campaign `ACTIVE` but `TCR_PENDING`, through REST and the MCP tool | `403 a2p_not_registered` on both | S | `core::phone::a2p::readiness`, `it::phone::a2p_gate_all_paths` |
| P69 | Marketing to France at 23:00 Paris time; transactional at the same time | `409 quiet_hours` with `next_allowed_at`; sent | S | `core::phone::catalogue::quiet_hours`, `it::phone::quiet_hours` |
| P70 | Segment caps reached; `TenantQuota` unreachable | `429 daily_cap_reached` with the cap; `503 unavailable`, never a pass | S | `it::phone::sms_caps_fail_closed` |
| P71 | A destination country not allowed; a premium-rate or short-code destination | `403 policy_denied` (`destination_country`); `403 destination_blocked` | S | `it::phone::sms_destinations` |
| P72 | `message.sent` from a different sender than requested | `sender_used` recorded, `sender_substituted: true`, metric | S | `it::phone::sender_substitution` |
| P73 | A spend cap reached; the cap raised; the same key retried | `402 spend_cap_reached` with nothing stored; then accepted | S | `it::phone::spend_cap` |
| P74 | Monthly charges that exceed the cap | Charged; `phone_spend.warning` at 100%; numbers kept | S | `it::phone::recurring_over_cap` |
| P75 | A cost-bearing event delivered twice | One charge | S | `it::phone::charge_unique_source` |
| P76 | A quote read by a tenant key and by a partner key | GBP only; GBP and `quote.provider` | S | `it::phone::quote_visibility` |
| P77 | The invoicing job runs twice for a month | One Stripe invoice item per tenant and category (Stripe idempotency key) | S | `it::phone::invoicing_idempotent` |
| P78 | A partner statement | Only that partner's tenants | S | `it::phone::partner_statement_scope` |
| P79 | Provider balance under 7 days of spend; under 2 days | Page alert; `paused_purchases` automatically, SMS and calls continue | S | `it::phone::balance_low` |
| P80 | The provider's daily spend limit hit (40333) | `failed` (`provider_spend_limit`); `phone_provider_limit_hit` | S | `it::phone::provider_limit` |
| P81 | Brand `VETTED_VERIFIED`; an unknown status | Approved; unchanged with `phone_status_unknown` | S | `core::phone::telnyx::a2p_status_table` |
| P82 | Brand creation repeated after a crash | The existing brand adopted by tag or EIN; no second fee; campaigns by `referenceId` | S | `it::phone::a2p_no_double_fee` |
| P83 | `10dlc.campaign.update` reaches `/hooks/telnyx`; `MNO_PROVISIONED` | Mapped; numbers assigned and ready | S | `it::phone::a2p_events` |
| P84 | A simulated ID written for a live tenant | Refused by the table `CHECK` | S | `it::phone::a2p_no_sim_ids` |
| P85 | A `vapi` route | Vapi credential once per connector; number adopted or created; PATCH then GET must match, else `voice_configuration`; no Telnyx key sent to Vapi | S | `it::phone::vapi_route`, `live::phone::vapi_route` |
| P86 | Vapi answers `401` | Route `failed`; tenant notified | S | `it::phone::vapi_revoked` |
| P87 | An outbound call under `approval` | An `outbound_call` request; on approval placed once with the disclosure first; an unknown outcome `uncertain` and reconciled from Vapi's call list | S | `it::phone::outbound_call_flow` |
| P88 | A call to an emergency or premium number; policy `off`; a non-`vapi` route | `403 destination_blocked`; `403 policy_denied` (`calls_off`); `409 voice_route_unsupported` | S | `it::phone::outbound_call_refusals` |
| P89 | A `forward` route to the tenant's own number, or to a disallowed country | Refused at request creation (`400 invalid_request`) | S | `it::phone::forward_rules` |
| P90 | Call events and call records; an outbound call the service did not place | `calls` rows and `call.*` events; usage counts as caps count; `phone_unplaced_outbound_call` and a notification | S | `it::phone::call_records`, `it::phone::unplaced_call` |
| P91 | A US number without an emergency address | Its routes place no calls; an approved `enable_emergency` request registers one | S | `it::phone::us_emergency` |
| P92 | A port-in the provider splits; an exception; a cancel within 48 hours of FOC | One `pti_` and number row per draft; `action_needed`; `409 port_in_not_cancelable` | S | `it::phone::port_in` |
| P93 | A port-out request | `port_out_pending`, page alert, urgent emails, first in "Needs a person", `respond_by` 24 hours, escalations at 50%, 75%, 90% | S | `it::phone::port_out_notice` |
| P94 | Nobody decides a port-out | Never rejected automatically; the provider's authorisation completes it; `ported_out`, route removed, PIN deleted, history kept | S | `it::phone::port_out_auto_authorised` |
| P95 | A port-out rejected (code 1001 without text; with text) | `400 invalid_request`; back to `active` and the PIN rotated | S | `it::phone::port_out_reject` |
| P96 | Nightly: a tagged provider number with no row; a live row missing at the provider; configuration drift | `phone_orphan_number`, never deleted; `suspended` (`missing_at_provider`); repaired with read-back | S | `it::phone::nightly_reconcile` |
| P97 | A webhook delivery that failed at the provider | Found in `webhook_deliveries`; the resource re-read; counted | S | `it::phone::missed_webhooks` |
| P98 | Tenant erasure with numbers, open requests, documents and campaigns | `phone_release` cancels, releases, deactivates, deletes resources and connectors, shreds documents except retained ones, listed in the receipt | S | `it::phone::tenant_erasure` |
| P99 | KYC after the last number; a platform export | Deleted after `PM_PHONE_KYC_RETENTION_DAYS`; export behind a single-use link with an audit row | S | `it::phone::kyc_retention_and_export` |
| P100 | Adopting a number already in the directory or stamped for another tenant; a legacy-group number | Refused; adopted with `legacy_group`, a `legacy_deadline` and notifications at 60, 30 and 7 days | S | `it::phone::adopt` |
| P101 | The ops switch at `paused_purchases` and `paused`; `PM_PHONE` turned off and on | Refusals as § 30; no number released; on re-enable the reconciler runs first | S | `it::phone::switch_and_rollback` |
| P102 | `put_managed` fails at activation | The number stays `configuring`; retried; `phone_pin_missing` after an hour | S | `it::phone::pin_vault_unavailable` |
| P103 | Every phone route with keys of another tenant, another identity and another partner | The same `404` body as a missing resource | S | `it::phone::scope_matrix` |
| P104 | A provisional agent sign-up | `phone:*` not effective: `403 permission_denied` (`signup_unverified`) | S | `it::phone::provisional_signup` |
| P105 | An opt-in recorded by an identity key; an opt-out by an identity key with `phone:send` | `403 permission_denied`; recorded | S | `it::phone::consent_permissions` |
| P106 | A dry-run send | `would_send`, segments and price; no hold and no row | S | `it::phone::sms_dry_run` |
| P107 | Pylota moves from its own Telnyx integration | Its handler takes Pylota Identity's signed events keyed on `evt_`; it sends no STOP confirmation; its old Telnyx key and `TELNYX_*` environment are retired | I | Pylota's `phone-identity-cutover` suite |
| P108 | A purchase covered by the standing budget; the same purchase with the budget off | Created `approved` with `approved_by = "standing_budget"` and `budget_version`, executed at once, audited; with the budget off, `pending_approval` with `details.outside_budget = "disabled"` | S | `core::phone::budget::covers`, `it::phone::buy_within_budget` |
| P109 | Each budget rule exceeded in turn (country, type, numbers per identity, month count, setup ceiling, monthly ceiling, `expiring_only`) | `pending_approval` with `details.outside_budget` naming the first failed rule | S | `core::phone::budget::covers` |
| P110 | Two covered purchases race for the month's last slot | One is covered; the other is `pending_approval` (`month_count`); no slot is counted twice | S | `it::phone::budget_last_slot` |
| P111 | An identity key or a tenant key loosens the budget; a tenant key tightens it; a partner key under `phone.key_approval` loosens it with `on_behalf_of` | `403 permission_denied` (`person_required`); allowed; allowed and audited | S | `it::phone::budget_who` |
| P112 | `POST …/phone-numbers` with `country: "GB"` inside the budget | `201` with the number (type mobile) once the order is accepted; configuration continues in the queue; a replay with the same key returns the same `201` | S | `it::phone::buy_one_call` |
| P113 | The one-call purchase whose inline run exceeds 15 seconds; outside the budget; blocked on onboarding | `202` with the request, the run continuing in the queue; `202` `pending_approval` with the quote and `follow_on`; `202` with `blocked_by` and `next_step` | S | `it::phone::buy_one_call_202` |
| P114 | An exact number taken (85001); `any` with the first three candidates taken; `area_code` on GB | `409 phone_number_taken` with up to 5 alternatives and quote tokens; the fourth candidate ordered; `400 invalid_request` (`location_not_supported`) | S | `it::phone::buy_taken_alternatives` |
| P115 | A US number before the tenant's campaign is `MNO_PROVISIONED`, then after | `outbound_sms: registration_required` (or `activating` once submitted), `setup_state: registration_pending`; then `enabled`, `ready`, `phone_number.capability_changed` and `phone_number_ready` | S | `core::phone::capabilities::of`, `it::phone::capability_facets` |
| P116 | A facet says `enabled` while the send gate would refuse | Impossible: both read `core::phone::capabilities::of`; a property test over every combination of facets, registrations and rows | S | `core::phone::capabilities::agrees_with_gates` |
| P117 | A US purchase quote for a tenant without 10DLC, with it, and for a GB-registered tenant | `follow_on` lists `register_us_a2p` with fee and wait; omits it; lists `not_available` (`brand_country_not_supported`); the digest changes with `follow_on` | S | `it::phone::quote_follow_on` |
| P118 | A blocked purchase whose group is approved later; one never unblocked | Unblocked in the group's batch and executed if approved; expires at 30 days | S | `it::phone::blocked_purchase` |
| P119 | `register_us_a2p` on a tenant at its monthly cap; above the registration cap | Accepted against the registration cap; `402 spend_cap_reached` with `details.scope = "registration"` | S | `it::phone::registration_cap` |
| P120 | The setup status for GB ready, US not started, US unavailable at the platform | Statuses and steps as specified; `next_step` names the person and the console page; `unavailable` with `platform_prerequisite` | S | `core::phone::onboarding::status`, `it::phone::setup_status` |
| P121 | An agent drafts registration text with a tax ID field, a link in the description, and a valid draft | `400 invalid_request`; stored as escaped text, never a link; stored; a person submits it and the request is approved in the same step; the draft deleted | S | `it::phone::onboarding_draft` |
| P122 | A purchase with `expires_at`; an agent extends it; an agent shortens it; release at the time with a pending port-out | Release request created `approved_by = "purchase"` at `release_at`; `403 permission_denied`; allowed; the release waits and retries, never skipping a rule | S | `it::phone::expiring_number` |
| P123 | A US-registered profile without `company_registry.region`; a requirement unmapped for the registration country | `422 compliance_incomplete` (`company_registry`); `422 compliance_incomplete` with `details.missing[]`, `phone_requirement_unmapped` | S | `core::phone::catalogue::document_for`, `it::phone::registry_mapping` |
| P124 | A GB-registered tenant's US number; its `register_us_a2p`; its call with no US address | `outbound_sms: not_available` (`brand_country_not_supported`), buy needs `acknowledge_not_available`; `422 compliance_incomplete` (`brand_country_not_supported`); `outbound_calls: not_available` (`no_us_emergency_address`) | S | `it::phone::non_us_tenant_us_number` |
| P125 | SMS from a US number to a GB handset; from a GB number to a US handset before the cross-border test | `403 policy_denied` (`destination_not_supported`); `403 policy_denied` (`destination_untested`) | S | `it::phone::cross_border_destinations`, `live::phone::cross_border_sms` |
| P126 | A legacy-group number of a sole-trader tenant: before its deadline (never before 2027-01-24), at it, after a port-out | Works, reminders only; `suspended` (`legacy_unpapered`), inbound kept, outbound `409 phone_number_not_ready`, never released; `ported_out` | S | `it::phone::legacy_exit_path` |
| P127 | The provider account not at the Verified level | US rows `eligible: false` (`platform_prerequisite`) in the catalogue and the setup status; a US purchase blocked on `platform_prerequisite` | S | `it::phone::account_level_probe` |
| P128 | A send while the ops switch `phone` is at `paused_purchases` (set by hand or by low provider balance); the same send at `paused` | Sent at `paused_purchases`; `503 unavailable` (`phone_paused`) at `paused`; a purchase at `paused_purchases` is refused | S | `it::phone::sms_under_paused_purchases` |
| P129 | A Worker dies after the spend intent and before the order's outcome is stored; the lease lapses | The next claim takes the `executing` request, reconciles the attempt by `customer_reference` first, adopts a found order or proves absence, and never orders twice | S | `it::phone::stale_executing_reclaimed` |
| P130 | A purchase on a row with `pre_approval: false` (Italy) with a complete but unapproved group, then the group approved or declined | Ordered with the group; the number stays `under_review` with no facet configured; activated after approval; `failed` (`regulatory_rejected`) when declined | S | `it::phone::order_first_review` |
| P131 | Approved requests of each kind while the ops switch is at `paused_purchases` and at `paused` | `outbound_call` and `release_number` run at `paused_purchases`; only `release_number` at `paused`; every other kind waits and runs once the switch is `on` | S | `it::phone::switch_by_kind` |

## 35. Tests

The `it::phone::*` tests are named against their rows in [§34](#34-edge-cases); each runs against
`phone/fake.rs` with fault injection unless it says otherwise. The tests below add the pure unit tests and
the **contract tests** that check every external behaviour this design relies on against the real
providers (one real-provider test per external contract: Telnyx and Vapi documents disagree with themselves,
and a fake only confirms what its author believed).

| Test | Proves | Covers |
|---|---|---|
| `core::phone::e164` | Country codes, lengths, normalisation; refusals | §12.1 |
| `core::phone::segments` | GSM-7 and UCS-2 segment counts, with the disclosure prepended, at every boundary (property test) | §12.1, §12.2 |
| `core::phone::catalogue::rows_valid` | The shipped catalogue parses; every row has sources and a `verified_at` within 180 days; quiet-hours dates exist for this year and next | §3 |
| `core::phone::requests::digest` | Canonical JSON and digest stability; any change of payload or quote changes it | §5.3 |
| `core::phone::numbers::transitions` | Every allowed transition of §6.2 and the refusal of every other | §6.2 |
| `core::phone::telnyx::status_tables` | Every Telnyx enum value read (orders, requirements, groups, numbers, porting, port-outs, 10DLC, toll-free) maps as specified; an unknown value maps to "unknown" | §2.2, §6.2, §13 |
| `core::phone::orders::absence_proved` | Absence only from two complete, empty listings at the required spacing | P25, P27 |
| `core::phone::sms::check` | The pipeline order of §12.2 and each refusal | P64–P71 |
| `core::phone::sms::quarantine` | The rule order of §10.2, including 2b | P50–P52, P58–P60 |
| `core::phone::verify::find_code` | The email rule's patterns without links; solicitation window | P57, P62 |
| `core::phone::money::price` | The rate-card formula, rounding up, the higher of conflicting prices | §18.1 |
| `live::phone::webhook_signature` | A real Telnyx test event verifies over `timestamp|raw body`; a modified byte fails | §9.2 |
| `live::phone::order_contract` | On staging with the operator's account: search, order with `customer_reference`, filter orders by it, read states, release; the reference's length limit | §6.3 |
| `live::phone::order_listing_lag` | The time from order creation to its appearance in the reference listing; fails if above the 5-minute floor | §6.3 |
| `live::phone::send_idempotency` | Two `POST /messages` with one `Idempotency-Key` send one SMS and return the same ID; gates the repeats of §12.3 | §12.3 |
| `live::phone::redaction_levels` | What each messaging-profile redaction level keeps | §8, §24 |
| `live::phone::external_pin` | PIN format rules and read-back | §16 |
| `live::phone::a2p_paths` | The `/v2/10dlc/*` paths and the brand and campaign fields used | §13 |
| `live::phone::mms_media_hosts` | The hosts inbound MMS media are served from | §10.4 |
| `live::phone::forwarding` | The number voice-settings fields for forwarding | §14.4 |
| `live::phone::vapi_route` | Bring-your-own SIP trunk credential, number, translated number, an inbound test call reaching the assistant | §14.2 |
| `live::phone::vapi_call_list` | Listing a Vapi number's calls by time and customer number | §14.5 |
| `live::phone::stripe_invoice_item` | An invoice item in GBP with an idempotency key lands on the next invoice (Stripe test mode) | §18.5 |
| `core::phone::budget::covers` | Every rule of the standing budget and the order they are checked in | §5.7 |
| `core::phone::onboarding::status` | The setup status from every combination of profile, group, registration and platform facts | §4.7 |
| `core::phone::capabilities::of` | The four facets from every combination of row, facets, registrations, emergency address and legacy state | §6.4 |
| `live::phone::uk_brand_registration` | A GB-registered business's brand and campaign registered on staging; the identifier TCR accepts in place of an EIN | §13.2 |
| `live::phone::sole_prop_otp` | A mock sole-proprietor brand: code triggered, verified, campaign created. Built only if a later decision allows sole traders (§13.1 is not offered) | §13.1 |
| `live::phone::us_onboarding_end_to_end` | A real US business onboarded: brand, campaign, a number assigned, a text delivered; the days each step took | §2.5, §4.7 |
| `live::phone::cross_border_sms` | `gb_mobile_to_us` and `us_local_to_gb`: a text each way with its receipt | §3.5 |
| `live::phone::e911_address` | An emergency address registered on a US number; a non-US address refused | §14.8 |
| `live::phone::toll_free_registration_types` | The `businessRegistrationType` values Telnyx accepts per registration country | §4.3 |
| `live::phone::account_level` | The probe that tells the Verified level apart | §2.1 |
| `it::mcp::phone_tools` | The twelve tools are listed only with their permissions; there is no approving tool; `phone_buy_number` has no confirmation argument | §28 |
| `cli::phone::commands` | Against recorded fakes: `setup phone`, `phone setup status`, `phone budget set` (prompts before loosening), `phone buy`, `search`, `request number`, `requests approve` (prompts without `--digest`), `sms send` with a derived key, `documents upload` | §28 |
| `xtask::phone_catalogue` | A release fails when a catalogue row's `verified_at` is older than 180 days | §3.1 |

## 36. Milestones

To add to the [build plan](../build-plan.md) (in `build-plan.md` these headings are level 2). Production keeps
`PM_PHONE` off until M41's acceptance passes, because the port-out guard and the nightly reconciler are part of
what makes a live number safe to hold.

### M38 · Phone numbers, requests and compliance (after M5, M6, M8, M14, M17 Foundation, M21, M22 and M27)

M5 for the router, `key_may_decide` and the `TenantQuota` stubs; M6 for identities; M8 for platform events and
fan-out; M14 for the erasure and retention jobs; M17 Foundation for metrics and alerts; M21 for the console;
M22 for holds and Stripe; M27 for the service ledger that §11.2 extends.

**Files:** `crates/core/src/phone/{mod.rs, e164.rs, catalogue.rs, requests.rs, budget.rs, onboarding.rs,
capabilities.rs, numbers.rs, orders.rs, money.rs, telnyx.rs}`, `crates/core/data/{phone-catalogue.json,
phone-rate-card.json}`, `crates/worker/src/phone/{provider.rs, telnyx/*, fake.rs, execute.rs, buy.rs,
onboarding.rs, orders.rs, numbers.rs, tenant_resources.rs, compliance.rs, documents.rs, money.rs}`,
`handlers/phone/{catalogue.rs, setup.rs, budget.rs, buy.rs, numbers.rs, requests.rs, compliance.rs, charges.rs,
platform.rs}`,
`handlers/hooks_telnyx.rs`, `consumers/phone.rs` (kinds `provider_event`, `execute`, `reconcile`, `configure`),
`crons/phone.rs`, `console/pages/phone/{setup.rs, budget.rs, numbers.rs, requests.rs, compliance.rs, charges.rs}`,
`crates/cli/src/commands/phone.rs` (setup, countries, setup status and drafts, budget, buy, search, request,
requests, numbers, compliance, documents, charges). D1 tables of [Data model](data-model.md) for numbers, releases, requests, profiles, documents,
requirement groups, tenants, provider events and charges.

| File (owner milestone) | Hook |
|---|---|
| `deploy/wrangler.toml.tmpl`, `lib.rs` (M5) | The binding `Q_PHONE`, queues `pm-phone` and `pm-phone-dlq`, the R2 lifecycle rule for `phone-staging/`; an exception to "no milestone adds a binding", as M31's `Q_SCREEN` is |
| `quota/mod.rs` (M5, M22) | `QuotaRequest::PhoneSpendHold`, `PhoneSpendSettle`, `PhoneSpendRelease` (the hold taking the budget slot `phone_budget:t:*` with the spend in one transaction) and the counters `phone_spend:*`, `phone_spend:reg:*`, `phone_budget:t:*`, `phone_search`; an exception to "no new variants", as M35's is |
| `auth/people.rs` (M5) | `Decision::ApprovePhoneRequest`, `Decision::DecidePortOut`, `Decision::SetPhoneBudget` |
| `auth/routespec.rs`, `router.rs` (M5) | The phone routes and the seven permissions |
| `core/src/sealed.rs` (M17) | The sealed columns of §23 |
| `jobs/erasure.rs`, `jobs/retention.rs` (M14) | `phone_release`, the document steps, receipt fields; retention of requests, events, index and charges |
| `billing/*` (M22) | The `phone_spend` hold feature |
| `handlers/identities.rs` (M6) | `409 identity_has_phone_numbers` |
| `observability/catalogue.rs` (M17) | The metrics and alerts of §29 |
| `mcp/tools.rs` (M15) | `phone_list_numbers`, `phone_search_numbers`, `phone_buy_number`, `phone_setup_status`, `phone_get_request` |

**Implements:** FR-PHN-1 to FR-PHN-20, FR-PHN-22 (order events), FR-PHN-39 (number charges and caps), FR-PHN-40,
FR-PHN-41 (its tools), FR-PHN-43, FR-PHN-44, FR-PHN-45 (the facets that need no registration), FR-PHN-46,
FR-PHN-47, FR-PHN-49 (GB onboarding and the status route), FR-PHN-50, FR-PHN-51, FR-PHN-53, and edge rows P1–P41,
P73–P76, P98, P99, P101, P103, P104, P108–P114, P117, P118, P120–P123, P127, P129–P131.

**Order.** The first pull request is `live::phone::order_contract` and `live::phone::webhook_signature` with the
pure mapping tables, so the status tables rest on the real API before any state machine is built.

**Acceptance:** the rows above with their tests; `it::phone::scope_matrix` joins
`it::security::cross_tenant_matrix`; `sdk::coverage::every_operation` passes with the new operations.

### M39 · SMS (after M38, M30 and M31)

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
onboarding, drafts for 10DLC), FR-PHN-52 (texting from US numbers and cross-border destinations), and edge rows P42–P72, P77 (SMS charges in the job's input),
P80–P84, P105, P106, P115, P116, P119, P124 (SMS half), P125, P128.

**Acceptance:** the rows above; `live::phone::send_idempotency` decides whether the repeats of §12.3 are enabled;
`live::phone::redaction_levels` recorded in §8; `live::phone::a2p_paths`, `live::phone::sole_prop_otp`,
`live::phone::cross_border_sms` and `live::phone::us_onboarding_end_to_end` pass on staging (the last gates step 5
of [§2.5](#25-us-enablement), and its measured waits replace the catalogue's `typical_wait_days`);
`live::phone::uk_brand_registration` is run and its result recorded, whatever it is.

### M40 · Voice routes and calls (after M38)

**Files:** `crates/core/src/phone/voice.rs`, `crates/worker/src/phone/{voice.rs, vapi.rs}`,
`handlers/phone/{routes.rs, connectors.rs, calls.rs}`, `mailbox/calls.rs`, CLI `connectors`, `route`, `calls`,
`usage`; MCP `phone_place_call`, `phone_list_calls`.

**Implements:** FR-PHN-32 to FR-PHN-35, FR-PHN-52 (calls from US numbers for tenants registered elsewhere) and
edge rows P85–P91, P124 (calls half).

**Acceptance:** the rows above; `live::phone::vapi_route`, `live::phone::vapi_call_list`,
`live::phone::forwarding` and `live::phone::e911_address` pass on staging.

### M41 · Porting, port-out guard, reconciliation, invoicing and migration (after M38, M39, M40 and the vault core M42)

**Files:** `crates/core/src/phone/porting.rs`, `crates/worker/src/phone/{porting.rs, port_out.rs, reconcile.rs,
invoicing.rs}`, `handlers/phone/port_outs.rs`, `jobs/phone_reconcile.rs`, `console/pages/phone/port_outs.rs`,
the adopt route, CLI `port-outs`, `reconcile`, `statement`, `numbers adopt`.

**Implements:** FR-PHN-36 to FR-PHN-38, FR-PHN-39 (invoicing and statements), FR-PHN-42, FR-PHN-54 (the recorded
decision and the legacy exit path), and edge rows P77–P79, P92–P97, P100, P102, P126. A backfill step sets a vault
PIN on every number activated before this milestone.

**Acceptance:** the rows above; a staging rehearsal of Pylota's migration (adopt or port a test number, send and
receive, place an approved call, release); `live::phone::stripe_invoice_item`; seven days of the telemetry of
§30 on staging before `PM_PHONE` is turned on in production; the sole-trader refusal enforced (ADR 0020 decision 15);
no adopted number left without a lawful path (each legacy number has a `legacy_deadline` or a recorded
re-papering).

## 37. Compared with AgentPhone

AgentPhone facts were first read on 2026-10-24 from its public site, documentation, OpenAPI documents and
packages, and re-read on 2026-10-26 from its published artifacts: npm `agentphone` 1.0.25 (the TypeScript SDK,
whose `reference.md` carries the server's endpoint descriptions), PyPI `agentphone` 0.20.1 (the Python SDK),
and the source of `github.com/AgentPhone-AI/agentphone-mcp` at `a78c5df` (v0.8.0), all MIT-licensed; nothing was
run. They are untrusted data and are used only to compare. AgentPhone is never a supplier
([§1](#1-what-a-phone-number-is-here)).

| Topic | AgentPhone | Pylota Identity |
|---|---|---|
| Countries | US and Canada self-serve; UK mobile SMS-only "on accounts where available"; no EU | GB, US, and an EU catalogue with local-presence rules |
| Buying | `POST /v1/numbers` with every field optional (country default US, area code, agent, `expiresAt`), paid from a prepaid balance | `POST …/phone-numbers` with the country required and the type defaulted per country ([§6.7](#67-one-call-purchase)) |
| Approval | The MCP tool `buy_number` buys only with `confirm: true`, which the agent passes itself; the REST route has no confirmation | The standing budget or a person decides on the server; `phone_buy_number` has no confirmation flag ([§5.7](#57-the-standing-purchase-budget)) |
| A taken number | `409` "with fresh alternatives in this same shape" | The next candidate for `any`; `409 phone_number_taken` with alternatives for an exact number |
| Regulatory identity | None collected for numbers; UK numbers sold at once | Each tenant's own profile and groups, done once per country in onboarding ([§4.7](#47-phone-onboarding)) |
| US texting | One registration per account (sub-account per registration): `sole_proprietor` or `standard_brand` $25, `high_volume` $70; "typically 7-10 business days" | One registration per tenant, every later number joins its campaign; fees at cost in GBP under a registration cap ([§13](#13-us-a2p-10dlc-and-toll-free-verification), [§18.3](#183-spend-caps)) |
| What a number can do | `outboundSms`: `enabled`, `activating`, `registration_required` | The same three values plus `not_available`, for four capabilities, with a reason and a next step ([§6.4](#64-readiness-capabilities-and-pylotas-failure-codes)) |
| Throwaway numbers | `expiresAt` auto-releases | `expires_at`, the release approved with the purchase and run with every safety rule ([§6.8](#68-throwaway-numbers)) |
| Status for agents | The MCP tool `account_overview` lists agents, numbers and the webhook and says what to do next | `phone_setup_status` per country, with the person who must act ([§4.7](#47-phone-onboarding)) |
| Idempotency | None on sends or calls; its TypeScript SDK retries `POST`s | Required keys everywhere; attempt-scoped references; provider keys on SMS; `uncertain` never resent |
| Opt-out | Not exposed | One profile per tenant; a ledger per brand |
| Verification codes | Not extracted; a separate Verify API sends codes "through carrier-registered sender IDs" because "plain application SMS from a US long code is not deliverable" to some countries, the UK among them | Extracted; ledger-gated; vault-only channel; cross-border destinations gated by tests ([§3.5](#35-sending-across-borders)) |
| Port-out | Not offered | PIN in the vault; alerts; a person decides |
| Erasure | By email request | Scopes with receipts, KYC included; processor settings |
| Agent ergonomics | `llms.txt`, Markdown docs, plain failure reasons, test webhooks | Kept: narrow tools, plain reasons with `retryable`, the guide written for agents ([Phone guide](../../guides/phone.md)) |

## 38. Integration

The shared pages carry this page's contracts: the PRD, the REST API reference and `openapi.yaml`, the errors,
events, limits, MCP, CLI and configuration references, the data model, architecture, security, privacy,
billing, workspace policy, the service sign-up ledger, the event feed, notifications, observability, the
edge-case register and the build plan. A
difference between this page and one of them is a defect: fix both in the same change.

## 39. Open questions for the owner

Gathered here so they can be answered in one pass. Items 1, 2, 9 and 10 are decided; 11 and 12 block M38 or M41 as marked.

1. **Decided (2026-10-26):** the rate card's 40% margin and 10% currency buffer, whole-month charging (no
   proration), and regulatory pass-through fees at cost with no margin.
2. **Decided (2026-10-26):** the Cloud default spend caps (£20 a tenant, £10 an identity, £60 for registration
   fees a month), and phone only on a paid plan for self-serve workspaces.
3. Whether Pylota Identity Cloud's Telnyx account is Pylota's existing TREFT account (numbers adopted, no
   porting) or a new one (numbers ported) ([§7.6](#76-adoption-for-pylotas-migration)). The owner's report that
   GB is set up on TREFT's account points to adoption.
4. The account's data locality (recommended: EU), set once and irreversible.
5. The Telnyx Verified level for TREFT: step 1 of US enablement ([§2.5](#25-us-enablement)); apply now.
6. Questions to ask Telnyx: emergency address registration for GB and EU numbers, and for US numbers of a
   tenant with no US location; whether numbers move between managed accounts without porting; whether each
   managed account has its own signing key; the released-number hold period; the redaction levels; the
   `customer_reference` length and listing lag; whether TCR vets GB-registered brands and with which
   identifier.
7. Whether to offer GB geographic numbers before the address conflict is settled (designed:
   `requires_local_presence`).
8. The sequencing of section letters: the edge-case section is `P`, which the register's preamble reserves
   in spirit to avoid confusion with priorities `P0`–`P2` (as `U` for the vault collides with the selling
   points `U1`–`U10`); the coordinator may prefer another letter.
9. **The standing budget** ([§5.7](#57-the-standing-purchase-budget)): the amended rule is **approved**
   (2026-10-26), and so is the default budget Pylota sets for its operators: GB and US, 2 numbers per identity,
   10 a month, £3 setup and £3 a month per number.
10. **Sole traders** ([§1.1](#11-sole-traders-refused-owner-decision-2026-10-26)): **decided**, refused (option (c), 2026-10-26).
    Still useful before M41: count the adopted numbers it affects, so their owners hear of the 2027-01-24 exit
    date early.
11. Whether a US number may be sold to a tenant registered outside the US for receiving and inbound calls only
    while GB brands are unproved (designed: yes, with `acknowledge_not_available`).
12. The default lifetime limits for throwaway numbers (designed: 1 hour to 365 days) and whether expiring
    numbers should be the only kind a budget covers by default (designed: no).
