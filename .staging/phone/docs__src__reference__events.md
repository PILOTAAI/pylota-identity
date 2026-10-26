Where this goes: `docs/src/reference/events.md`. (1) A new subsection of "## Event types" after "### Service accounts"; (2) two sentence changes in "### Ordering". Source: [Phone numbers § 25](../project/design/phone-numbers.md#25-events-and-audit).

## 1. New subsection, after "### Service accounts" (after its table)

### Phone numbers

**Mailbox events** carry the identity's `sequence`, like `message.*`. SMS text and transcripts are untrusted
content; verification codes are never in an event (an SMS text shows them as `[code]`), and an SMS withheld for
the vault (`vault_code`) emits nothing. Counterparty numbers appear in `sms` and `call` only where a mail event
would carry an address; masked hints elsewhere.

| Type | When | `data` |
|---|---|---|
| `sms.received` | An inbound SMS is stored and visible | `sms` (summary below), `conversation_id`, `trust` (`sender_unverified: true`, `screening`), `media[]` (`n`, content type, size) |
| `sms.quarantined` | An inbound SMS is stored but held | As `sms.received` without `text`, plus `quarantine_reason` (`account_unapproved`, `otp_unsolicited`, `steering_suspected`, `screening_unavailable`, `risky_attachment`) |
| `sms.released` | A held SMS was released | `sms_id`, `released_by_key_id` or `released_by_user_id` |
| `sms.screened` | A screening verdict on an SMS | `sms_id`, `status`, `quarantine_reason`, `screening` |
| `sms.verification_received` | A verification code was found in a visible SMS | `sms_id`, `number_id`, `sender_hint`, `kind` (`code`), `account_id`. The value is only available through `GET …/sms/wait` |
| `sms.sent` | The provider accepted the SMS | `sms_id`, `segments`, `sender_used`, `sender_substituted` |
| `sms.delivered` | The handset or carrier confirmed delivery | `sms_id` |
| `sms.failed` | The SMS was refused or not delivered | `sms_id`, `reason` |
| `sms.delivery_unconfirmed` | The carrier gave no final answer | `sms_id` |
| `sms.uncertain` | The outcome is unknown; never resent automatically | `sms_id`, `reason`, `fix` |
| `sms.reconciled` | An uncertain SMS was matched to a provider record | `sms_id`, `status` |
| `call.started` | A call on one of the identity's numbers began | `call` |
| `call.ended` | It ended, or a call record filled it in later | `call` (with `duration_seconds`, `outcome`, `cost_pence`, `source`: `webhook` or `cdr`) |

The **SMS summary** in `data.sms`:

```json
{ "id": "sms_01JB…", "conversation_id": "smc_01JB…", "number_id": "pnm_01JB…", "direction": "inbound",
  "status": "received", "from": "+447700900456", "to": "+447700900123", "text": "Can I collect at 3?",
  "segments": 1, "client_reference": null, "received_at": "…", "sent_at": null, "flags": ["sender_unverified"] }
```

The **call** object: `id`, `number_id`, `direction` (`inbound`, `outbound`), `counterparty`, `route_kind`,
`started_at`, `answered_at`, `ended_at`, `duration_seconds`, `outcome` (`answered`, `no_answer`, `busy`, `failed`,
`rejected`), `disclosure_applied`, `cost_pence`, `source`.

**Platform events** have `sequence: null`, with `tenant_id` and `identity_id` set where the subject belongs to an
identity. `phone_request` is the [phone request](api.md#phone-requests) as the API returns it to the endpoint's
level (provider costs only to platform and partner endpoints).

| Type | When | `data` |
|---|---|---|
| `phone_request.requested` | A request was created, or returned to `pending_approval` because its price rose | `phone_request`, `reason` (`created` or `quote_changed`) |
| `phone_request.approved` | A person (or an allowed key) approved it | `phone_request`, `approved_by_key_id` or `approved_by_user_id`, `on_behalf_of` |
| `phone_request.rejected` | Rejected, or expired after 7 days | `phone_request`, `reason` (`operator` or `expired`) |
| `phone_request.canceled` | Canceled by the requester, or by erasure | `phone_request`, `reason` |
| `phone_request.completed` | The provider work ended successfully | `phone_request`, `result` |
| `phone_request.failed` | The provider work ended without success | `phone_request`, `reason` |
| `phone_request.uncertain` | A billable call's outcome is unknown; the service is reconciling | `phone_request` |
| `phone_number.status_changed` | A number changed status | `number_id`, `identity_id`, `from`, `to`, `setup_state`, `provisioning` (`status`, `code`, `retryable`), `reason` (for example `moved`, `adopted`, `released`, `missing_at_provider`) |
| `phone_number.action_needed` | The provider needs information before a deadline | `number_id`, `action_needed` (`kind`, `message` as untrusted text, `deadline_at`) |
| `phone_number.port_out_requested` | Another carrier asked for the number | `number_id`, `port_out_id`, `respond_by` |
| `phone_number.port_out_decided` | A person authorised or rejected it, or the provider authorised it | `number_id`, `port_out_id`, `decision` (`authorized`, `rejected`, `provider_authorized`), `decided_by_key_id` or `decided_by_user_id` |
| `phone_registration.status_changed` | A US 10DLC brand, campaign or number assignment, or a toll-free verification, changed status | `registration_id`, `kind`, `from`, `to`, `failure_reason` |
| `phone_compliance.status_changed` | The compliance profile's status for a country, number type and action changed | `country`, `number_type`, `action`, `from`, `to` (no personal data) |
| `phone_consent.changed` | A counterparty opted out or in | `number_hint` (masked), `state`, `purpose`, `source` |
| `phone_spend.warning` | Phone spend reached 80% or 100% of a monthly cap | `scope`, `identity_id`, `percent`, `used_pence`, `cap_pence`, `resets_at` |

## 2. In "### Ordering"

- In the sentence listing per-identity mailbox events, add `sms.*` and `call.*` after `quota.warning`.
- In the sentence listing platform events with `sequence: null`, add `phone_request.*`, `phone_number.*`,
  `phone_registration.*`, `phone_compliance.*`, `phone_consent.changed` and `phone_spend.warning`.

## 3. In "## Reading events without an endpoint", add after "A key sees an event only when it could read its subject through the API"

: SMS and call events need `phone:read` (held SMS events also `quarantine:review`), request, number,
registration, consent and spend events need `phone:read`, and compliance events need `phone:compliance`
