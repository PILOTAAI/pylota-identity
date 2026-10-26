Where this goes: `docs/src/project/prd.md`. Five insertions, each introduced below by the heading or row it follows.

## 1. In "## 5. Scope and priorities", add this row to the table after the row "Plans and billing"

| Phone numbers (behind `PM_PHONE`, [ADR 0020](adr/0020-phone-numbers.md)) | Catalogue with GB mobile, GB geographic and US local rows; compliance profiles and encrypted write-only KYC documents; phone requests approved by people; ordering with spend intents, attempt-scoped references and reconciliation; numbers on identities with history; release; per-tenant provider resources; signed provider webhooks; inbound and outbound SMS with the consent ledger, caps and the US 10DLC gate; verification codes with the ledger gate and the vault channel; the `vapi` voice route and approved outbound calls with disclosure; emergency settings; the port-out guard; the nightly reconciler; GBP rate card, spend caps, Stripe invoice items and partner statements; adoption for Pylota's migration (FR-PHN-1–23, 25–35, 37–42) | Ireland, Netherlands, US toll-free and the local rows for Germany, France, Spain and Italy; SMS screening by the classifier (FR-PHN-24); `sip` and `forward` routes; toll-free verification; port-in (FR-PHN-36) | Call Control and media streaming routes; alphanumeric sender IDs; outbound MMS; managed provider accounts per partner; a second provider (Twilio) |

## 2. In "### Non-goals (v1.0)", add these bullets after the bullet "Attachments fetched from URLs."

- Becoming a telephony carrier or holding number ranges from a regulator. Numbers are bought from a provider
  (Telnyx) in each tenant's own legal name ([Phone numbers](design/phone-numbers.md)).
- Phone numbers for individuals or sole traders, and numbers bought by an agent without a person's approval.
- Call Control webhooks and media streaming through Pylota Identity (planned for v1.1); alphanumeric sender IDs;
  outbound MMS; WhatsApp, RCS and iMessage.

## 3. In "## 6. Functional requirements", add this subsection after "### 6.22 Agent self-sign-up" (after FR-ASU-16)

### 6.23 Phone numbers

Behind `PM_PHONE` ([ADR 0020](adr/0020-phone-numbers.md), [Phone numbers](design/phone-numbers.md)). The
priority of each requirement is given in brackets; acceptance names the edge rows of section P of the
[edge-case register](edge-cases.md).

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
  name its numbers are registered; individuals and sole traders **must** be refused in v1; tax identifiers and
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
  request, created with a required `Idempotency-Key`, bounded per tenant, and rejected as expired after 7
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
- **FR-PHN-14** (P0) Numbers **must** follow the state machine of [Phone numbers § 6.2](design/phone-numbers.md#62-the-provisioning-state-machine), with carrier, SMS and voice facets each
  read back from the provider, `setup_state` and `provisioning { status, code, retryable }` using Pylota's
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
  the sending identity, and the ordered pipeline of [§ 12.2](design/phone-numbers.md#122-policy-pipeline), whose caps fail closed; a dry run **must** check
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
- **FR-PHN-39** (P0) Money **must** follow [Phone numbers § 18](design/phone-numbers.md#18-money): GBP prices from the rate card, one charge per unique source,
  monthly spend caps per tenant and identity enforced with holds and refused with `402 spend_cap_reached` before
  anything is stored, recurring charges never refused, provider-side daily limits as backstops, balance alerts
  with an automatic pause, Stripe invoice items with idempotency keys, partner statements, and a paid plan or a
  partner for self-serve tenants. Acceptance: P73–P80.
- **FR-PHN-40** (P0) Erasure scopes **must** cover SMS, media, calls and phone data with the receipt fields of
  [§ 24](design/phone-numbers.md#24-privacy); tenant erasure **must** run `phone_release`; logs **must** pseudonymise phone numbers; every sealed column
  **must** be in the sealed-column registry. Acceptance: P53, P98, P61.
- **FR-PHN-41** (P0) The MCP server **must** offer the eleven `phone_*` tools of [§ 28](design/phone-numbers.md#28-mcp-and-cli) and no tool that approves,
  releases, ports, submits KYC, changes a route or shows a PIN; the CLI **must** offer `idvy phone …` and
  `idvy setup phone`. Acceptance: `it::mcp::phone_tools`, `cli::phone::commands`.
- **FR-PHN-42** (P0) A platform key **must** be able to adopt a number already in the provider account into an
  identity, stamping ownership and refusing a number already held; an adopted number registered under a
  platform group **must** be re-papered under its tenant's profile within 90 days. Acceptance: P100.

## 4. In "## 10. Risks", add these rows to the table after the last row

| Telephony is regulated, and TREFT may count as a provider (Ofcom General Conditions, emergency calling, caller ID) | Numbers registered in each tenant's own name; agent routes never dial emergency numbers; tenants warned before purchase; each legal question listed for counsel and answered before the feature opens in production ([Phone numbers § 32](design/phone-numbers.md#32-questions-for-counsel)) |
| One Telnyx account serves every tenant: a suspension stops all of them, and limits are shared | Per-tenant provider-side spend limits; the `phone` ops switch; balance alerts; `provider_account_id` on every row so a partner can move to its own managed account; a provider trait that admits a second provider |
| Provider costs are in US dollars and customers pay in pounds | A rate card with a currency buffer; an alert at 5% exchange-rate drift; 30 days' notice before existing numbers are re-priced |
| Telnyx's documents contradict themselves on prices, statuses and rules | Every external behaviour has a contract test against the real API before code depends on it; the catalogue fails a release when a row is unverified for 180 days |

## 5. In "## 13. Business model and pricing", add this paragraph after the paragraph that starts "- Prices are in pounds sterling (GBP) and exclude VAT."

- **Phone numbers** are priced outside the plan allowances: a monthly and a one-time price per number, and
  per-segment and per-minute usage, all in GBP from a published rate card built from the provider's USD list
  prices with a margin and a currency buffer ([Phone numbers § 18](design/phone-numbers.md#18-money)). They
  need a paid plan, or a partner that pays for its tenants, and are capped per workspace and per identity each
  month. Charges are added to the next Stripe invoice, or to the partner's monthly statement.
