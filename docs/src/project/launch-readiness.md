# Launch readiness

The one page that says whether Pylota Identity Cloud can go live, product by product: mail, phone numbers,
the vault, the SDKs, the console, the site and the docs. Every row is a gate item with the party that closes
it, the evidence that proves it, what it blocks, and its status. The build plan says how each piece is built;
[Cloud commissioning](cloud-commissioning.md) says how production is configured and checked; this page
collects the items from both, and the ones that are not code at all (legal pages, counsel, tax, the licence,
providers), so that nothing that a launch needs lives only in someone's head.

| | |
|---|---|
| Requirement | FR-OPS-6, NFR-OPS-4 ([PRD](prd.md#613-operations)) |
| Built in | [M49 · Launch](build-plan.md#m49--launch-after-m28-m41-m45-and-m48), after M28, M41, M45 and M48 |
| Related | [Cloud commissioning](cloud-commissioning.md) (checks CC1–CC33, gates A–D, P, V, S), [Security › Pre-release checklist](design/security.md#16-pre-release-checklist), [Observability](design/observability.md), [Privacy](design/privacy.md), [Billing](design/billing.md), [Phone numbers](design/phone-numbers.md), [Agent credential vault](design/vault.md), [ADR 0015](adr/0015-solo-operator.md) |
| Status as of | 2026-10-26: design phase. `MILESTONES` lists `M0` only |

## 1. How this page is used

- **Status** is one of `open`, `in progress`, `blocked` (with what blocks it), `done` (with the date and the
  evidence link) or `n/a` (with why). The owner updates a row in the same change that closes it; a release
  that changes a row's evidence updates the row.
- **Evidence** is something a later reader can check without asking: a test name, a check ID of
  [Cloud commissioning §9](cloud-commissioning.md#9-checks), a published URL, a signed document stored in the
  private operations repository (named, never copied here), or a dated note.
- **Owner** is one of: *owner* (TREFT LTD's director, the only operator), *counsel*, *accountant*, *Telnyx*,
  *Stripe*, or a build milestone (`M…`), meaning the item closes when that milestone's acceptance passes.
- **Blocks** names the gate of [Cloud commissioning §10](cloud-commissioning.md#10-go-live-gates) the item
  holds closed: `A`, `B`, `C`, `D` (mail and sign-up), `P1`–`P3` (phone stages), `V1`–`V2` (vault stages),
  `S` (Python and Go SDKs), or `v1.0` (the release tag).
- `cargo xtask release-gate` refuses to promote `v1.0.0` while any row that blocks `v1.0` is not `done` or
  `n/a` (`xtask::launch_readiness_refuses_open_blockers`), and `idvy doctor` on production warns while a
  row that blocks a gate whose flag is on is not `done` (`cli::doctor::launch_readiness_gates`).

## 2. Order of launch

```text
 build M0 … M37 ─▶ M20 staging proof ─▶ v1.0.0 ─▶ M28 gate A (Pylota only) ─▶ gate B (waitlist) ─▶ gate C (open) ─▶ gate D (agent sign-up)
                                                       │
 build M38 … M41 (phone) ───────────────────────────────┼─▶ gate P1 (Pylota's first operator, GB) ─▶ P2 (all Pylota operators) ─▶ P3 (others, after C)
                                                       │          └─ US numbers join each stage only after §3.3 rows PH-6 … PH-10
 build M42 … M45 (vault) ───────────────────────────────┼─▶ gate V1 (Pylota's tenants and consumers) ─▶ V2 (others, after C)
                                                       │
 build M46 … M48 (Python and Go SDKs) ──────────────────┴─▶ gate S (first full SDK releases)
                                                                       ▼
                                                              M49 launch: every row below done or n/a
```

Mail goes first because Pylota's integration needs it and the other two products are built on its
identities, ledger, policy and console. Phone and the vault each open to Pylota's tenants before anyone
else's, and to other workspaces only once open sign-up (gate C) has run. Each step back is one flag or one
switch ([§10](#10-rollback-and-kill-switches)).

## 3. Gate tracker

### 3.1 Build

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| BLD-1 | Mail milestones M0–M37 accepted | M0–M37 | `MILESTONES` lists each; `cargo xtask trace` green for them | A | in progress (M0) |
| BLD-2 | Staging proof and release candidate | M20 | M20 accepted; `release-gates/v1.0.0.md` ticked, naming the candidate | A, v1.0 | open |
| BLD-3 | Every `S` and `S+I` edge row of the register has a passing test | M20 | `cargo xtask trace` with `M20` in `MILESTONES` | v1.0 | open |
| BLD-4 | Phone milestones accepted | M38–M41 | M41's staging rehearsal and seven days of phone telemetry ([Phone numbers §36](design/phone-numbers.md#36-milestones)) | P1 | open |
| BLD-5 | Vault milestones accepted | M42–M45 | M45's acceptance, including the consumer conformance fixtures | V1 | open |
| BLD-6 | Python and Go SDK milestones accepted | M46–M48 | Each SDK's contract tests and live round trip on staging | S | open |
| BLD-7 | Launch milestone accepted | M49 | Every row of this page `done` or `n/a` for the gates opened | — | open |

### 3.2 Mail, console, site and docs

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| MAIL-1 | Production configured and checked | owner (M28) | CC1–CC14, CC18, CC19 | A | open |
| MAIL-2 | Fresh-agent dry run and adversarial review of the production configuration | owner | Reports in the operations repository | A, C | open |
| MAIL-3 | Pylota on Cloud for 14 days with no service-owned edge row failing | owner | Dated note with the alert history | B | open |
| MAIL-4 | Stripe live mode, OAuth clients and the waitlist | owner, Stripe | CC15, CC16, CC17 | B | open |
| MAIL-5 | DMARC ramp on `pylotamail.com` | owner | CC18 at `p=quarantine` (B), `p=reject` (C) | B, C | open |
| MAIL-6 | Thirty days of phase B within the auto-pause thresholds | owner | Metrics export | C | open |
| MAIL-7 | Agent self-sign-up live tests on production | owner | `live::signup::end_to_end`, `live::signup::x40_refuse_one_click_headers` | D | open |
| CON-1 | Console covers phone and vault decisions: approval queues, compliance, charges, vault items, grants, uses, audit and reveal | M38–M45 | The console tests the phone and vault milestones name | P1, V1 | open |
| SITE-1 | The landing page describes only what is live: phone and vault sections carry "coming soon" until their gate opens, then are switched in the same change that opens it | owner (M19) | The site commit linked from the gate note | A, P1, V1 | open |
| SITE-2 | Site and docs use "source available" or "Fair Source", never "open source", in copy (an anchor ID may keep the old word) | M19 | `xtask::docs_forbidden_claims` extended to the site's copy | A | open |
| DOC-1 | Guides for phone numbers and the vault published, with the vault's trust statement word for word | M41, M45 | `mdbook build docs` with no warnings; CC32 | P1, V1 | open |
| DOC-2 | Reference pages carry every phone, vault and SDK contract (REST, `openapi.yaml`, events, errors, MCP, CLI, configuration, limits) | M38–M48 | The OpenAPI and SDK coverage tests (`sdk::coverage::every_operation`) | P1, V1, S | open |

### 3.3 Phone numbers

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| PH-1 | GB ordering on the Telnyx account | owner, Telnyx | Owner's statement of 2026-10-26; CC21 | P1 | done for GB (2026-10-26, owner); CC21 open |
| PH-2 | Reseller requirements annex accepted by TREFT | owner | Signed copy in the operations repository | P1 | open |
| PH-3 | Data locality set and `PM_TELNYX_API_BASE` matching | owner | CC21; the portal setting dated | P1 | open |
| PH-4 | Which Telnyx account Cloud uses (adoption or porting of Pylota's numbers) | owner | ADR 0020 amendment or note | P1 | open |
| PH-5 | Rate card margin and buffer, Cloud spend caps, registration-fee cap | owner | The rate card and default policy in the release | P1 | open |
| PH-6 | Telnyx account at the Verified level | owner, Telnyx | CC21 records the level | US in any stage | open |
| PH-7 | 10DLC set up as an ISV; `live::phone::a2p_paths` on staging | M39, owner | CC24 | US in any stage | open |
| PH-8 | A GB-registered brand taken through 10DLC vetting on staging | owner | Vetting result dated | US for UK tenants | open |
| PH-9 | E911 for tenants with no US location answered by Telnyx | owner, Telnyx | Written answer | US calls | open |
| PH-10 | Sole traders: decision on GB numbers and on adopted numbers of sole-trader operators | owner, counsel | ADR 0020 amendment | P1 (adoption), P3 | open |
| PH-11 | End-to-end GB and US numbers on production | owner | CC23, CC24 | P1 (GB), US | open |
| PH-12 | Spend limits, balance alert and kill switches rehearsed | owner | CC25, CC27 | P1 | open |
| PH-13 | Port-out guard live on every number | M41 | CC26; the PIN backfill step of M41 reports zero numbers without a PIN | P1 | open |
| PH-14 | Seven days of telemetry per stage | owner | [Phone numbers §30](design/phone-numbers.md#30-rollout-telemetry-and-rollback) metrics export | P2, P3 | open |

### 3.4 Vault

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| VLT-1 | Partner-held unlock key decision recorded | owner | ADR 0021 | V1 | open |
| VLT-2 | Pylota's consumers registered, conformance fixtures passed, activated by a platform key | M45, owner | `consumer.activated` audit rows with their notes | V1 | open |
| VLT-3 | Vault end to end on production | owner | CC28, CC29 | V1 | open |
| VLT-4 | Kill switches and `revoke-all` rehearsed | owner | CC30 | V1 | open |
| VLT-5 | Master-key rotation with vault rows | owner | CC31 | V1 | open |
| VLT-6 | Trust statement published; no forbidden claim | M45, owner | CC32 | V1 | open |
| VLT-7 | Independent adversarial review of the vault and of each consumer | owner | Report with nothing rated high open | V1 | open |
| VLT-8 | Thirty days of V1 with no exposed value, no audit append failure and no consumer suspension | owner | Metrics export | V2 | open |

### 3.5 SDKs and packages

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| SDK-1 | Package names held by TREFT LTD: crates.io crates, npm `@pylotahq/identity`, PyPI `pylota-identity`, the Go module repository | owner | Account pages; all four unpublished on 2026-10-26 ([Cloud commissioning](cloud-commissioning.md)) | v1.0, S | open |
| SDK-2 | Trusted publishers configured, token publishing disallowed | owner | [Cloud commissioning §13](cloud-commissioning.md#13-sdk-publishing); CC33 | S | open |
| SDK-3 | Licence of each SDK decided and stated in its package metadata | owner | ADR 0022 | S | open |
| SDK-4 | First full SDK releases resolve from each registry | M46–M48 | CC33 | S | open |

## 4. Legal pages and processors

Each page is written by counsel for TREFT LTD, published on the site before the gate it serves, and versioned
(`PM_TERMS_VERSION` stores the terms' date on each user). On 2026-10-26 `https://pylotamail.com/terms`
answered `404`: none is published yet.

| ID | Page | URL | Must cover | Owner | Blocks | Status |
|---|---|---|---|---|---|---|
| LEG-1 | Terms of service | `PM_TERMS_URL` (`/terms`) | Plans and billing, acceptable use (mail abuse, unlawful automation), suspension, liability; for phone, end users are businesses and the tenant's own legal name is used, and the reseller annex's flow-down terms; for the vault, the trust statement and the refusals | counsel | A (mail), P1, V1 | open |
| LEG-2 | Privacy policy | `PM_PRIVACY_URL` (`/privacy`) | What Cloud processes as controller (accounts, billing, waitlist) | counsel | A | open |
| LEG-3 | Data processing agreement | `PM_DPA_URL` (`/dpa`) | Cloud as processor for workspace content; international transfers; the sub-processor list by reference; KYC records under the reseller annex | counsel | A, P1 | open |
| LEG-4 | Sub-processor list | `/subprocessors` | The table below, with a 30-day change notice | owner | A | open |
| LEG-5 | Acceptable use policy for phone | part of LEG-1 | Marketing consent, quiet hours, no spoofing, no emergency calls through agent routes | counsel | P1 | open |
| LEG-6 | Trademark notice | `/trademarks` | "Pylota" and "Pylota Identity" are TREFT LTD's marks; the licence's trademark clause; how forks must rename | owner | A | open |

**Sub-processors**, from [Privacy › processors](design/privacy.md#3-jurisdiction-and-residency) and the phone
and vault designs. The page lists each with its location and when it applies:

| Sub-processor | Applies | Processes |
|---|---|---|
| Cloudflare | Always | Everything the service stores and runs, Workers AI included |
| Amazon Web Services (SES, S3, SNS, SQS, `eu-west-2`) | Domains on SES, marketing mail | Message content |
| Stripe | Paid plans, phone charges | Billing contacts and payment details |
| Google, GitHub | Sign-in with them | The person's verified address and account ID |
| Telnyx | Phone numbers | Numbers, SMS content, call records, KYC documents and the compliance profile |
| Vapi | Tenants that route calls to their own Vapi organisation | Call audio and metadata under the tenant's own Vapi contract; the deployment passes the route |
| Google Public DNS and RDAP servers | Domain checks | Domain names only |

## 5. Counsel and accountant

Questions whose answer opens a gate. The phone questions are numbered as in
[Phone numbers §32](design/phone-numbers.md#32-questions-for-counsel); the vault's are those of the vault
design's questions for counsel.

| ID | Question | Owner | Blocks | Status |
|---|---|---|---|---|
| CNS-1 | Phone 1: regulated provider status of TREFT under Ofcom's General Conditions (and the EU regulators before EU rows) | counsel | P1 | open |
| CNS-2 | Phone 2: emergency calling duties; the $100 unregistered-call charge | counsel | US calls, P1 for GB calls | open |
| CNS-3 | Phone 3: caller ID presented through Vapi's infrastructure | counsel | GB outbound calls | open |
| CNS-4 | Phone 4: AI disclosure and consent for automated calls and texts | counsel | P1 | open |
| CNS-5 | Phone 5: recording consent | counsel | `phone.recording` on for any tenant | open |
| CNS-6 | Phone 6: KYC record retention, lawful basis, roles of TREFT, the tenant and Telnyx, transfers | counsel | P1 | open |
| CNS-7 | Phone 7: sub-allocation under UK rules for numbers in the tenant's name in TREFT's account | counsel | P1 | open |
| CNS-8 | Phone 8: marketing SMS consent evidence and STOP handling | counsel | Marketing SMS | open |
| CNS-9 | Phone 9: port-out refusal grounds and PIN requirements | counsel | P1 | open |
| CNS-10 | Phone 10: excluding sole traders, and the migration of sole-trader operators | counsel | P1 (adoption), P3 | open |
| CNS-11 | Phone 11: US 10DLC registration by a UK ISV and of UK businesses | counsel | US texting for UK tenants | open |
| CNS-12 | Vault: every question of the vault design's questions for counsel (using credentials at third-party services on a person's behalf, refused services, records of use) | counsel | V1 | open |
| TAX-1 | VAT on the SaaS plans: UK registration date set in Stripe Tax | accountant | B | open |
| TAX-2 | Phone 12: VAT place of supply for telecommunications resold to UK and EU businesses; whether Stripe Tax covers it | accountant, counsel | P1 | open |
| TAX-3 | US numbers sold to US businesses: whether state telecommunications taxes or 911 fees apply to TREFT | accountant | US in P3 | open |

## 6. Licence and contributions

The service is under FSL-1.1-ALv2 ([PRD §12](prd.md#12-licensing)); the SDKs, the crate
`pylota-identity-api-types` and the agent skills are under Apache-2.0 ([ADR 0022](adr/0022-sdk-languages.md)
decision 9). Each release obliges:

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| LIC-1 | The right licence file in every published artefact: `LICENSE.md` (FSL-1.1-ALv2) in the Worker bundle, the CLI archives and the FSL crates; `LICENSE` and `NOTICE` (Apache-2.0) in the SDK crates, the npm and PyPI packages, the Go module and the skills | M19, M46–M48 | `xtask::release_artefacts_carry_licence` | v1.0, S | open |
| LIC-2 | `license = "FSL-1.1-ALv2"` (SPDX) in the service crates' metadata; `license = "Apache-2.0"` on `pylota-identity` (the Rust SDK) and `pylota-identity-api-types`, and in each SDK's manifest | M0, M16, M46–M48 | `Cargo.toml` (the two crates still inherit FSL from the workspace: switch them in M46); each SDK's manifest; `cargo deny check licenses` allowing both | v1.0, S | open |
| LIC-3 | A third-party licence file for each binary and package, generated from its SBOM at release | M19 | `THIRD_PARTY_LICENSES` in each archive; `cargo deny check licenses` green | v1.0 | open |
| LIC-4 | The release date of each version recorded, because each version becomes Apache-2.0 on its second anniversary | M19 | The GitHub Release date of each tag; the CHANGELOG states the date per version | v1.0 | open |
| LIC-5 | Contributions signed off under the DCO, as CONTRIBUTING.md requires | M19 | A CI job that refuses a pull request commit without `Signed-off-by` | A | open |
| LIC-6 | Trademark use: the licence grants no right to the marks; LEG-6 published | owner | LEG-6 | A | open |

## 7. Security sign-off

Every change to money, KYC, credentials, tenant isolation and auth is T2. Before each gate:

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| SEC-1 | The pre-release checklist complete for the release | owner | `release-gates/v{version}.md` | v1.0 | open |
| SEC-2 | External penetration test of mail, console and API | owner | Report; findings rated high fixed and retested | C | open |
| SEC-3 | External penetration test scope extended to phone (webhook signature, request approval, KYC document upload and access) and the vault (consumer authentication, ticket redemption, reveal, cross-tenant reach) | owner | Report | P3, V2 | open |
| SEC-4 | Independent adversarial agent review of each T2 surface before its gate (phone, vault, each consumer) | owner | Reports in the operations repository | P1, V1 | open |
| SEC-5 | Cross-tenant matrix includes every phone and vault route | M38–M45 | `it::security::cross_tenant_matrix`, `it::phone::scope_matrix` | P1, V1 | open |
| SEC-6 | Secret rotations rehearsed: master key with vault rows, Telnyx key, consumer keys | owner | CC31; dated notes | P1, V1 | open |
| SEC-7 | `SECURITY.md` scope names phone and vault, and the bounty-free disclosure process | owner | `SECURITY.md` | P1, V1 | open |

## 8. Operations for one operator

Pylota Identity Cloud has one operator ([ADR 0015](adr/0015-solo-operator.md)). Nothing below assumes a rota.

| ID | Item | Owner | Evidence | Blocks | Status |
|---|---|---|---|---|---|
| OPS-1 | Two alert channels and the external heartbeat (NFR-OPS-3) | M17, owner | `live::ops::heartbeat_workflow`; a test page received | A | open |
| OPS-2 | Runbooks for phone (provider auth, balance, uncertain orders, port-out, webhook gap) and for the vault ("Vault incident", exposed value, consumer compromise, master key) in [Observability §9](design/observability.md#9-runbooks) | M41, M45 | The runbook pages; each alert links one | P1, V1 | open |
| OPS-3 | Restore drill (NFR-OPS-2) on staging, and a D1 Time Travel restore with vault and phone rows, re-applying erasures | owner | `live::ops::restore_drill`; dated note | A, V1 | open |
| OPS-4 | Status page on a host outside the deployment's Cloudflare account, updated by hand during incidents, linked from the site and docs | owner | The URL | C | open |
| OPS-5 | Support channel: a mailbox outside `pylotamail.com` with a stated response time; `PM_SECURITY_CONTACT` separate | owner | Published on the site | B | open |
| OPS-6 | Incident response: a written order of actions (contain with a switch, assess, notify), the ICO notification duty within 72 hours of a personal data breach, and notice to affected workspaces | owner, counsel | Runbook "Personal data breach" | A | open |
| OPS-7 | Cover for owner absence: the break-glass record, a trusted person who can turn the ops switches off but cannot read data | owner | [Observability §5.7](design/observability.md#57-break-glass-record) record dated | A | open |
| OPS-8 | Provider status subscriptions: Cloudflare, AWS, Stripe, Telnyx | owner | Subscriptions to the operator's address | A, P1 | open |

## 9. Service levels

The objectives are those of [PRD §7](prd.md#7-non-functional-requirements) and
[Observability §4](design/observability.md#4-service-level-objectives), with the phone and vault rows of
NFR-REL-5, NFR-REL-6, NFR-PERF-7 and NFR-SEC-3. Cloud publishes them as targets, not as a contractual SLA,
until a paid plan carries service credits (owner decision; not in v1.0).

## 10. Rollback and kill switches

| Product | Fastest stop | Without a deploy | Full rollback |
|---|---|---|---|
| Sign-up | `PM_SIGNUP=closed` | `idvy ops switch agent_signup off` for agent sign-up | `npx --yes wrangler@4.139.0 rollback` |
| Pylota (partner) | — | `PATCH /v1/partners/{partner_id}` `status: suspended` | — |
| Phone | `PM_PHONE=off` (numbers kept at the provider, reconciled when on) | `idvy ops switch phone paused_purchases` or `paused` | Code rollback; tables are additive ([Phone numbers §30](design/phone-numbers.md#30-rollout-telemetry-and-rollback)) |
| Vault | `PM_VAULT=off` (hidden codes stay hidden) | `idvy ops switch vault_redeem off`; `revoke-all` per tenant; consumer suspension | Code rollback; never deletes items |

A switch is rehearsed on staging before its gate (CC27, CC30) and once on production in the first week.

## 11. Launch day and the first week

Launch day for any gate:

1. Check that every row blocking the gate is `done`; record the release tag.
2. Turn the flag or phase on in one deploy, alone; nothing else ships that day.
3. Run the gate's checks again against production within an hour.
4. Watch the alert email and the dashboard Overview for four hours; the heartbeat stays green.
5. Write the gate note.

The first week after each gate: a daily read of the SLO page, the `phone_*` and `vault_*` alert history, DLQ
depth, complaint and bounce rates, provider balance and spend, and the support mailbox. Any page alert in
that week halts the next stage until its cause is written down and fixed. On day 7 the owner records the
week's figures in the gate note and decides on the next stage.
