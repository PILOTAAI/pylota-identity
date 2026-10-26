Where this goes: `docs/src/project/architecture.md`. (1) Rows in "### Entry points" and "### Inbound sources and outbound transports"; (2) a new flow "### 4.8 Phone numbers" after "### 4.7 Notifications"; (3) a row in "## 8. External dependencies". Source: [Phone numbers](design/phone-numbers.md).

## 1. Entry points and transports

- Entry point: `POST /hooks/telnyx` (no key; Ed25519 signature over `telnyx-timestamp|raw body`) acknowledges
  provider events within 2 seconds by writing the raw event to R2 `phone-staging/` and queueing a pointer on
  `pm-phone`; it reads nothing from D1.
- Outbound transport: the `PhoneProvider` trait (`crates/worker/src/phone/provider.rs`), implemented for Telnyx at
  the fixed hosts `api.telnyx.com/v2` or `api.telnyx.eu/v2`, with Vapi (`api.vapi.ai`) for the `vapi` voice route.
- Queue: `pm-phone` (provider events, executions, reconciles, SMS dispatch, configuration) and `pm-phone-dlq`.

## 2. New flow, after "### 4.7 Notifications"

### 4.8 Phone numbers

```text
agent (identity key) ──POST …/phone-numbers {country}──▶ Worker
   │  catalogue row · onboarding status · standing budget · spend hold (TenantQuota)
   ├─ covered and onboarded ─▶ claim the request ─▶ spend intent (D1) ─▶ POST /number_orders (Telnyx) ─▶ 201 number
   ├─ outside the budget ─────▶ 202 request pending_approval ─▶ person approves (console / partner key) ─▶ queue
   └─ not onboarded ──────────▶ 202 request blocked_by ─▶ person finishes onboarding ─▶ unblocked ─▶ queue
Telnyx ──POST /hooks/telnyx──▶ R2 phone-staging + pm-phone pointer ─▶ consumer resolves the tenant from provider facts
   ├─ order and number events ─▶ phone_numbers state machine ─▶ configure (messaging profile, route, PIN in the vault)
   ├─ message.received ────────▶ IdentityMailbox IngestSms (quarantine rules, codes, vault channel)
   └─ receipts, 10DLC, port-outs ▶ SMS status, registrations, the port-out guard
cron (every minute) ─▶ due requests and numbers, expiring numbers, port-out listing
nightly reconciler ──▶ numbers, orders, porting, compliance, A2P, records, deliveries, opt-outs, balance
```

Every billable provider call is bracketed by Pylota Identity's own records (claim, spend intent,
attempt-scoped `customer_reference`), because Telnyx's order routes take no idempotency key; an unknown outcome
becomes `uncertain` and is reconciled before anything is retried. Tenants share one provider account and are
separated by their own messaging profile, connection, voice profile, billing group, requirement groups and tags.

## 3. External dependencies

| Telnyx | Phone numbers, SMS, voice, compliance, porting, US A2P | Behind `PM_PHONE`; one account in v1; contract tests against the live API before code depends on a behaviour |
| Vapi | The `vapi` voice route and outbound calls | Per-tenant connectors with the tenant's own Vapi key, sealed |
