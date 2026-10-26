# 0020 Phone numbers for agents, through Telnyx, approved by people

| | |
|---|---|
| Status | Proposed |
| Date | 2026-10-25; amended 2026-10-26 (decisions 4, 12 to 15) |
| Deciders | The owner (TREFT LTD) |
| Related | [Phone numbers](../design/phone-numbers.md), FR-PHN-1 to FR-PHN-54, edge rows P1–P131, milestones M38–M41, [ADR 0015](0015-solo-operator.md), [ADR 0019](0019-pylota-identity.md), the [vault design](../design/vault.md) (ADR 0021, written in parallel) |

## Context

The owner decided on 2026-10-24 that Pylota Identity gives agents phone numbers that can be obtained in the
United Kingdom, Europe and the United States ([ADR 0019](0019-pylota-identity.md)). Its first customer, Pylota,
already runs its own Telnyx integration: one platform Telnyx account, one GB mobile or US local number per
operator, one messaging profile for all operators, Vapi for voice, and a durable provisioning state machine
with spend intents and recovery by `customer_reference` (reviewed privately on 2026-10-24). The same
review listed lessons that this design builds in.

The facts the decision rests on, each read on 2026-10-24:

- **Telnyx** publishes an API-level compliance model (requirements, requirement groups, documents, addresses)
  and managed accounts for platforms. Managed accounts need a commitment plan ($1,000 to $2,000 a month on its
  pages; **Conflict**: $500 on another), and are not available on pay-as-you-go. US and EU numbers, 10DLC and
  toll-free verification need the Verified account level, which includes KYC of the reseller. Orders, porting
  orders, requirement groups and documents have no idempotency key, and Telnyx says not to retry a timed-out
  purchase before reconciling; `POST /messages` takes an optional `Idempotency-Key`. Opt-out block rules apply to
  a whole messaging profile. Port-outs are authorised automatically after 24–48 hours of silence. Telnyx bills
  in US dollars. (Telnyx OpenAPI `spec3.json` at `72cf10d738ff`, developer documentation, support articles,
  pricing API, reseller annex.)
- **Twilio** has the closest compliance model (bundles per country, number type and end-user type;
  subaccounts) and is the natural second provider. **Vonage** forbids onward allocation "except for use within
  their own customer application". **AgentPhone** forbids reselling raw access without a written agreement,
  offers no UK or EU numbers under the customer's identity, and has no regulatory API.
- National rules require identity and often local presence: Germany, France, Spain and Italy require end users
  to be physically present; Ireland and Italy require a requirement group on every order; French geographic
  numbers may not be sub-allocated since 2023-01-01.
- The reseller annex puts end-user KYC, record keeping, sub-allocation compliance and emergency-calling notice
  on the reseller, with an indemnity.

## Decision

1. **Provider.** Pylota Identity **must** buy numbers from Telnyx through a `PhoneProvider` trait, as mail goes
   through `MailTransport`, so a second provider (Twilio) is a second implementation. It **must never** use
   AgentPhone or Vonage, and **must not** become a carrier or hold number ranges.
2. **Tenancy.** v1 uses **one** Telnyx account owned by TREFT LTD (option A). Tenants are separated by their own
   messaging profile, SIP credential connection, outbound voice profile, billing group, requirement groups and
   tags. Every row naming a provider resource **must** store `provider` and `provider_account_id`, so moving a
   partner to its own managed account later needs no migration.
3. **Numbers belong to identities.** A number is an address of one identity; an identity may hold several and
   keeps its history when a number goes. Every number **must** be registered in the tenant's own legal name
   under the tenant's compliance profile; TREFT **must never** sub-allocate numbers registered in its name. v1
   serves business end users; sole traders wait on decision 15.
4. **People approve.** Agents may search and ask. Every purchase, port-in, release, KYC submission, US A2P fee,
   emergency registration, voice-route change and (by default) outbound call **must** be a phone request that a
   person approves (a purchase one by one or through a standing budget the person set, decision 12) through
   `key_may_decide` (with the platform-or-partner opt-in `phone.key_approval`), bound to
   a digest of the exact payload and GBP quote. Port-outs **must** be decided by a person and never rejected
   automatically.
5. **Safe retries.** Pylota Identity supplies its own idempotency on every route that can cost money, writes a
   spend intent with an attempt-scoped `customer_reference` before every order, makes a timed-out order
   `uncertain`, and reconciles before any further attempt. A nightly reconciler reads numbers, orders, porting,
   compliance, A2P, message and call records, webhook deliveries, opt-outs and the balance.
6. **SMS.** One messaging profile per tenant; a consent ledger per tenant brand; STOP, START and HELP answered by
   the provider's auto-responses only; inbound SMS signed (Ed25519, 5-minute window, every environment),
   acknowledged onto a queue, screened as untrusted input like mail; verification codes gated by the service
   ledger, and codes on a vault-claimed channel given only to the vault; one outbound send path with consent,
   caps, the US A2P gate for every sender, and the sender actually used recorded.
7. **Voice.** v1 routes are `vapi` (through Vapi's bring-your-own SIP trunk, never a Telnyx key in Vapi),
   `sip` (inbound) and `forward`; Call Control with media streaming waits for v1.1. Recording is off by default
   and needs an announcement; outbound calls carry an AI disclosure; agent routes never dial emergency numbers,
   and each country row has an emergency setting.
8. **Money.** Customer prices are GBP from a published rate card (provider USD × exchange rate × currency buffer
   × margin); monthly spend caps per tenant and per identity are enforced with holds in `TenantQuota`; charges
   are invoiced as Stripe invoice items or partner statements.
9. **Countries.** A data-driven catalogue; launch rows GB mobile (SMS and voice), GB geographic (voice), US local
   and toll-free, Ireland and the Netherlands; Germany, France, Spain and Italy only for tenants with local
   presence.
10. **Parity.** The design keeps every item of Pylota's parity list and builds in the lessons of the same review, so Pylota
    can move without losing anything.
11. **Legal questions** (regulated-provider status in the UK, emergency calling, Ofcom caller-ID rules, AI
    disclosure, recording consent, KYC retention, sub-allocation, marketing consent, port-out rejection, tax) are
    listed for counsel and **must** be answered before the corresponding feature is opened to tenants in
    production ([Phone numbers § 32](../design/phone-numbers.md#32-questions-for-counsel)).

12. **Standing purchase budget** (amends decision 4; approved by the owner on 2026-10-26). A
    person **may** approve purchases in advance through `phone.auto_approve`: off by default, set or loosened
    only by a decision reserved for people, bounded by country, type, numbers per identity, numbers per month
    and price per number, under the spend caps, and recorded by version on every purchase it covers. It covers
    `purchase_number` and the release a purchase schedules, nothing else.
13. **One-call purchase.** `POST /v1/identities/{identity_id}/phone-numbers` **must** buy a number in one call:
    the country required, the type defaulted per country (GB mobile, US local), `201` with the number inside the
    budget, `202` with the request otherwise, a taken number replaced or answered with alternatives, and an
    optional `expires_at` whose release the purchase's approval covers. The MCP tool `phone_buy_number` **must
    not** take a confirmation flag: the server decides.
14. **Onboarding once per tenant and country.** The regulatory work (the profile, documents, the requirement
    group, the US 10DLC registration, emergency addresses) is done once per tenant and country, reported as one
    setup status with the person who must act; agents may draft registration text and never submit it; a
    purchase that waits on onboarding is kept with `blocked_by`, not refused. Every number reports four
    capability facets (`enabled`, `activating`, `registration_required`, `not_available`) from the same
    function the send and call gates use, and a quote lists the registrations still to come.
15. **Sole traders: refused** (owner decision, 2026-10-26). Sole traders cannot hold numbers, for GB or US
    rows; no US sole-proprietor 10DLC brand is registered. A number adopted from Pylota's platform-wide group
    whose tenant is a sole trader keeps working with a dated exit (re-paper under an eligible business, or port
    out to a carrier the tenant chooses) no earlier than 2027-01-24, and is never released by a deadline. The
    options of allowing sole traders for GB and US rows, or for US rows only, were weighed and are kept in
    [Phone numbers §1.1](../design/phone-numbers.md#11-sole-traders-refused-owner-decision-2026-10-26) for a later decision.

## Risk tier, flag, telemetry and rollback

**T2.** The feature spends money on a prepaid provider account, holds KYC documents and identity data, routes
traffic between tenants on one shared provider account, and holds provider and voice-platform credentials. The
build needs a security review, an independent adversarial check that tries to break tenant isolation, approval
binding and spend safety, and the owner's sign-off before each milestone merges.

- **Flag:** `PM_PHONE` (`off` by default) and the per-tenant policy field `phone.allowed`; production keeps
  `PM_PHONE` off until M41 (the port-out guard and the nightly reconciler) has passed.
- **Kill switch:** the ops switch `phone` (`on`, `paused_purchases`, `paused`); it moves to `paused_purchases`
  automatically on a provider authentication failure or a low provider balance. It never releases a number.
- **Telemetry:** the metrics and alerts of [Phone numbers § 29](../design/phone-numbers.md#29-observability);
  each rollout stage waits seven days with no cross-tenant event mismatch, no stuck `uncertain` order, charge
  drift under 1%, no missed webhook after reconciliation, webhook acknowledgement p99 under 500 ms, and no
  port-out left undecided past half its window.
- **Rollback:** additive D1 tables and nullable columns only. Turning the flag off stops new work and keeps every
  number; turning it on again runs the reconciler first. Numbers are released only by an approved request or a
  tenant erasure.

## Consequences

- Pylota can retire its Telnyx and Vapi code paths and keep its behaviour; its operators gain per-operator
  opt-out isolation, their own regulatory identity, port-out protection and correct US registration.
- The service carries a fixed obligation: TREFT must reach Telnyx's Verified level, accept the reseller annex,
  keep KYC records for the locally mandated period, and keep the catalogue's sources current (a release fails on
  rows unverified for 180 days).
- One provider account is one point of failure: a suspension stops every tenant. Mitigations: per-tenant
  provider-side spend limits, the kill switch, balance alerts, and the trait that admits a second provider.
- Prices are exposed to the USD/GBP rate. Mitigations: a currency buffer in the rate card, an alert at 5% drift,
  and 30 days' notice before existing numbers are re-priced.
- Buying a number becomes one call once a tenant has onboarded a country and set a budget; the first number
  in a country still waits on the provider's review (about 72 hours in GB) or the US carriers' vetting.
- The build grows by four milestones (M38–M41), one queue binding (`Q_PHONE`, an exception to M5's "no new
  binding", as M31's was), three `TenantQuota` variants (an exception, as M35's was) and a new permission family.
- Follow-up work: the answers from counsel; the questions to Telnyx listed in the design; Call Control and media
  streaming (v1.1); managed accounts per partner when a commitment plan is justified; a second provider.

## Alternatives considered

- **AgentPhone underneath.** Fast to integrate and agent-friendly, but its terms forbid reselling without an
  agreement, it has no UK or EU numbers under the customer's identity, no erasure API and US-only processing.
- **Twilio first.** A comparable compliance model and subaccounts, but Pylota's working integration, numbers
  and operational knowledge are on Telnyx, and parity is cheaper there. Kept as the second provider.
- **Vonage, Bandwidth, Sinch.** Vonage's terms forbid onward allocation; Bandwidth's and Sinch's resale and
  UK/EU coverage were not established in the pages read.
- **One managed account per partner or tenant from day one (option B).** Better isolation (separate balances,
  keys, limits), but it needs a commitment plan of $1,000 to $2,000 a month before the first tenant pays.
  Designed for, not built.
- **Numbers registered in TREFT's name and sub-allocated.** Simplest KYC, but forbidden in France, doubtful
  elsewhere, and it makes TREFT the end user of every operator's number. Refused.
- **Agents buying within a budget they control, without approval.** Matches some agent products (AgentPhone's
  `buy_number` takes a `confirm` flag the agent passes itself), but purchases, ports and KYC submissions cost
  money, move personal data or cannot be undone, and an agent can be steered by the SMS it reads. Refused. The
  standing budget of decision 12 differs: a person sets it, the server enforces it, it covers purchases only,
  and it is bounded and audited.
- **Approval of every purchase one by one, with no budget.** Keeps a person in every purchase's path, so
  buying a number for an agent can never be one call. Replaced by decision 12, which the owner
  approved on 2026-10-26.
- **Refusing a purchase until onboarding is complete.** Leaves the agent at a dead end with nothing to hand a
  person. Replaced by `blocked_by` (decision 14).
- **Call Control as the v1 voice route.** It would give Pylota Identity the call events and recording control,
  but its 500 ms command-retry expectation, its command surface and its pricing make it a v1.1 item; Pylota does
  not use it today.
