Where this goes: `docs/src/project/design/observability.md`. (1) Rows in "### 3.2 Catalogue"; (2) rows in "### 5.3 Alert list"; (3) a runbook "### Phone provider" in "## 9. Runbooks"; (4) checks in "### 7.2 `idvy doctor`". Source: [Phone numbers § 29](phone-numbers.md#29-observability).

## 1. Metrics

Every metric named in [Phone numbers § 29](phone-numbers.md#29-observability), with labels as given there (never
numbers, tenant names or text).

## 2. Alerts

Page: `phone_port_out_pending`, `phone_port_out_deadline` (75% and 90%), `phone_provider_balance_low`,
`phone_provider_auth`, `phone_unplaced_outbound_call`, `phone_event_mismatch`. State: `phone_order_uncertain_stuck`,
`phone_reconcile_drift`, `phone_orphan_number`, `phone_status_unknown`, `phone_requirement_unmapped`,
`phone_review_long`, `phone_webhook_signature_failures`, `phone_fx_drift`, `phone_provider_limit_hit`,
`phone_pin_missing`, `phone_recording_undeclared`, `phone_release_unattributable`, `phone_order_duplicate`,
`phone_registration_slow`, `phone_expiring_release_stuck`.

## 3. Runbook

### Phone provider

- **Balance low**: top up the Telnyx account; purchases resume when the hourly check sees 7 days of cover
  (`paused_purchases` lifts by `idvy ops switch phone on` after the top-up).
- **Provider auth**: rotate `PM_TELNYX_API_KEY` in the portal and with `idvy setup phone`; the switch was moved to
  `paused_purchases` automatically.
- **Event mismatch**: a tenant-isolation incident. Stop with `idvy ops switch phone paused`, read the
  `ignored_mismatch` rows, and find which tenant resource is misconfigured before resuming.
- **Port-out pending**: make sure a person of the tenant has seen it; never reject on the tenant's behalf.
- **Uncertain stuck**: check the order in the Telnyx portal by `customer_reference`, then
  `POST /v1/platform/phone-requests/{id}/resolve`.

## 4. `idvy doctor`

`phone`: the account level probe (`platform_state.phone_account`), the webhook channel and a test event's
signature, the notification settings, the balance's days of cover, and whether US rows are on while the level is
not Verified (an error).
