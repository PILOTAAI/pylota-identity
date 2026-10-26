Where this goes: `docs/src/reference/api.md`. (1) A new section after "## Agent sign-up" (before "## Domains"); (2) rows in the permissions table of "### Permissions"; (3) one sentence in "### Idempotency". Source: [Phone numbers](../project/design/phone-numbers.md).

## 1. New section, after "## Agent sign-up"

## Phone numbers

Behind `PM_PHONE` and the tenant's `phone.allowed` (with the flag off every route below answers
`404 route_not_found`). A number is an address of one identity, registered in the tenant's own legal name. An
agent buys inside the tenant's standing budget, or asks; a person decides everything else
([Phone numbers](../project/design/phone-numbers.md), [guide](../guides/phone.md)). Every route that can cost
money requires `Idempotency-Key` (`x-idempotency: required` in [`openapi.yaml`](openapi.yaml)).

### `GET /v1/phone/countries` — `phone:read`

The catalogue rows the deployment offers, each with `country`, `number_type`, `status`, `capabilities`,
`sms_use`, `local_presence`, `default_for_country`, `destinations`, `registration` (fees and
`typical_wait_days`), `review_hours`, `emergency` and the tenant's own standing: `eligible`, or `reason`
(`local_presence_required`, `compliance_missing`, `not_allowed_by_policy`, `platform_prerequisite`,
`entity_type`).

### `GET /v1/tenants/{tenant_id}/phone-setup` · `GET /v1/tenants/{tenant_id}/phone-setup/{country}` — `phone:read`

Phone onboarding: one entry per country the tenant's policy allows ([Phone numbers § 4.7](../project/design/phone-numbers.md#47-phone-onboarding)).

```json
{ "country": "US", "status": "action_needed",
  "steps": [ { "kind": "platform_prerequisite", "status": "done" },
             { "kind": "profile", "status": "done" },
             { "kind": "register_us_a2p", "status": "not_started", "who": "person",
               "fee": { "setup_pence": 2839, "monthly_pence": 823 }, "typical_wait_days": [7, 15] } ],
  "after_setup": { "outbound_sms": "enabled", "inbound_sms": "enabled", "inbound_calls": "enabled",
                   "outbound_calls": "registration_required" },
  "next_step": { "who": "person", "kind": "register_us_a2p", "console_url": "https://console.example.com/phone/setup/US" } }
```

`status` is `not_started`, `in_review`, `action_needed`, `ready` or `unavailable` (with `reason`). Identity keys
may read it: it holds no personal data.

### `PUT /v1/tenants/{tenant_id}/phone-setup/{country}/draft` · `GET` · `DELETE` — `phone:request`

An agent's draft of the text a registration needs, for a person to review and submit. `Idempotency-Key`
optional. Body fields, all optional strings unless stated: `business_description`, `use_case`,
`sample_messages` (2–5, each 20–1,024 characters and containing the brand name), `consent_flow`, `website`,
`privacy_policy_url`, `terms_url`, `opt_in_image_url`. Any other field, a tax identifier or personal data is
`400 invalid_request`. Returns `200` with the draft (`pdr_`, `drafted_by_key_id`, `updated_at`). One draft per
tenant and country; deleted when its request is created or after 30 days untouched.

### `GET /v1/tenants/{tenant_id}/phone-budget` — `phone:read` · `PUT` — `phone:manage`

The standing purchase budget ([Phone numbers § 5.7](../project/design/phone-numbers.md#57-the-standing-purchase-budget)).

```json
{ "enabled": true, "countries": ["GB", "US"], "number_types": ["mobile", "local"],
  "max_numbers_per_identity": 2, "max_new_numbers_per_month": 10,
  "max_setup_pence_per_number": 300, "max_monthly_pence_per_number": 300, "expiring_only": false,
  "version": 4, "set_by": { "user_id": "usr_…", "key_id": null, "on_behalf_of": null }, "set_at": "…",
  "this_month": { "covered": 3, "resets_at": "2026-11-01T00:00:00Z" } }
```

`PUT` replaces the object (`Idempotency-Key` required). Tightening any field, or `enabled: false`, is allowed to
`phone:manage`. Setting it or loosening any field is a decision reserved for people: from an API key only a
platform key or the tenant's partner key where `phone.key_approval` is `true`, with `on_behalf_of`; otherwise
`403 permission_denied` (`person_required`) and a person saves it in the console. Identity keys get
`403 scope_denied`. Emits `phone_budget.changed`; audit `phone_budget.set`.

### `POST /v1/identities/{identity_id}/phone-numbers` — `phone:request`

Buy a number in one call ([Phone numbers § 6.7](../project/design/phone-numbers.md#67-one-call-purchase)).
`Idempotency-Key` required.

```json
{ "country": "US", "area_code": "512", "expires_at": null, "primary_for": ["sms"],
  "client_reference": "branch-austin" }
```

| Field | Rule |
|---|---|
| `country` | Required |
| `type` | Default the country's row that can text (`mobile` in GB, `local` in the US) |
| `area_code`, `contains`, `locality` | Optional; `400 invalid_request` (`location_not_supported`) on a type with no location, such as GB mobile |
| `number`, `quote_token` | An exact number from a search; otherwise the service picks |
| `expires_at` | Optional, 1 hour to 365 days ahead; the number is released then |
| `acknowledge_not_available` | `true` to buy even though a `follow_on` entry says a capability cannot be had for this tenant |

- **`201`**: the [number](#phone-number-object) once the provider accepted the order inside the tenant's
  standing budget. Configuration continues; follow `phone_number.status_changed` and
  `phone_number.capability_changed`.
- **`202`**: the [request](#phone-request-object) when a person must act (outside the budget, `details.outside_budget`),
  when the purchase waits on onboarding (`blocked_by`), or when the run continues in the queue. `next_step`
  names who must act.
- `409 phone_number_taken` with `details.alternatives[]` (exact numbers only); `402 spend_cap_reached`;
  `422 country_not_offered`, `local_presence_required`, `compliance_incomplete` (`entity_type`),
  `phone_number_limit_reached`, `phone_request_limit_reached`; `403 policy_denied` (`phone_not_allowed`,
  `plan_required`).

### `GET /v1/identities/{identity_id}/phone-numbers/available` — `phone:request`

Search: `country` and `type` (required), `contains`, `starts_with`, `ends_with`, `locality`, `region`, `limit`
(1–20). Each candidate has `phone_number`, `locality`, `region`, `features`, a GBP `quote` (with `follow_on`)
and a 30-minute `quote_token`. Results are not reserved. 300 searches a day per tenant (`429 daily_cap_reached`,
`details.cap = "phone_search"`).

### `POST /v1/identities/{identity_id}/phone-numbers/portability` — `phone:request`

`{ "numbers": ["+447700900456"] }`: the provider's portability check per number (`portable`, `fast_portable`,
`messaging_capable`, `reason`). Free, read-only, no approval.

### <a id="phone-number-object"></a>`GET /v1/identities/{identity_id}/phone-numbers` · `GET …/phone-numbers/{number_id}` — `phone:read`

```json
{ "id": "pnm_01JB…", "identity_id": "idn_01J9…", "phone_number": "+15125550123", "country": "US",
  "number_type": "local", "status": "active", "setup_state": "registration_pending",
  "capabilities": {
    "outbound_sms":   { "state": "registration_required",
                        "next_step": { "who": "person", "kind": "register_us_a2p",
                                       "console_url": "https://console.example.com/phone/setup/US" } },
    "inbound_sms":    { "state": "enabled" },
    "inbound_calls":  { "state": "enabled" },
    "outbound_calls": { "state": "registration_required",
                        "next_step": { "who": "person", "kind": "enable_emergency" } } },
  "primary_for": ["sms"], "provisioning": { "status": "ready", "code": null, "retryable": false },
  "requirements_status": null, "deadline_at": null, "action_needed": null, "expires_at": null,
  "port_pin": "set", "compliance_status": "own", "legacy_deadline": null,
  "active_from": "…", "active_until": null, "request_id": "phr_01JB…", "client_reference": "branch-austin" }
```

Each capability `state` is `enabled`, `activating`, `registration_required` or `not_available` (with `reason`).
`provider` (provider IDs) appears only for platform and partner keys. The list filters `status` (default the
live statuses).

### `PATCH /v1/identities/{identity_id}/phone-numbers/{number_id}` — `phone:manage` · `phone:request`

`{ "primary_for": ["sms","voice"], "expires_at": "…" }`. `primary_for` needs `phone:manage`. An earlier
`expires_at` is allowed to `phone:request`; a later one or `null` needs `phone:manage`.

### `POST /v1/identities/{identity_id}/phone-numbers/{number_id}/move` — `phone:manage`

`{ "to_identity_id": "idn_…" }` within the tenant. History stays with the old identity.

### `GET /v1/identities/{identity_id}/phone-numbers/{number_id}/voice-route` — `phone:read`

The number's route (`kind`, configuration without secrets, `status`, `vapi_phone_number_id`).

### `GET /v1/tenants/{tenant_id}/phone-numbers` · `GET /v1/phone-numbers/lookup?number=` — `phone:read`, tenant level and above

The tenant's numbers; and the identity and number IDs of a live number in the key's scope, or `404
phone_number_not_found` (also for another tenant's number).

### <a id="phone-request-object"></a>Phone requests — `phone:request`, `phone:read`, `phone:approve`

`POST /v1/identities/{identity_id}/phone-requests` (`Idempotency-Key` required) with `kind` and `payload`;
`GET` lists (filters `status`, `kind`); `GET …/phone-requests/{request_id}`; `GET
/v1/tenants/{tenant_id}/phone-requests` (the tenant's queue, oldest pending first).

```json
{ "id": "phr_01JB…", "kind": "purchase_number", "status": "pending_approval",
  "payload": { "selection": { "mode": "any", "country": "US", "type": "local", "area_code": "512" } },
  "quote": { "currency": "GBP", "max_setup_pence": 127, "max_monthly_pence": 127,
             "follow_on": [ { "capability": "outbound_sms", "kind": "register_us_a2p", "setup_pence": 2839,
                              "monthly_pence": 823, "typical_wait_days": [7, 15], "who": "person", "once_per": "tenant" } ],
             "rate_card_version": 3, "valid_until": "…" },
  "digest": "9f2c41ab…", "blocked_by": [], "approved_by": null, "budget_version": null,
  "details": { "outside_budget": "country" }, "expires_at": "…", "result": null }
```

`kind` is `purchase_number`, `port_in`, `release_number`, `submit_compliance`, `register_us_a2p`,
`verify_toll_free`, `enable_emergency`, `set_voice_route` or `outbound_call`. An identity key creates only
`purchase_number`, `port_in` (without secrets), `release_number` and `outbound_call` for its own identity.

- `POST …/phone-requests/{request_id}/approve` (`phone:approve`, `Idempotency-Key` required) with
  `{ "digest", "note"?, "on_behalf_of"? }`: a decision reserved for people (`403 permission_denied`,
  `person_required`, unless `phone.key_approval` and a platform or the tenant's partner key); never the
  requesting key (`self_approval`); `409 phone_request_not_pending`; `409 phone_request_digest_mismatch`
  with `details.current_digest`.
- `POST …/reject` (`phone:approve`) with `{ "reason" }`; `POST …/cancel` (the requester or `phone:approve`).
- `POST /v1/platform/phone-requests/{request_id}/resolve` (`platform:ops`) with `outcome` (`ordered` with the
  provider order ID, or `not_ordered`) ends an attempt left `uncertain` for 24 hours.

### Compliance — `phone:compliance`

- `GET`, `PATCH /v1/tenants/{tenant_id}/phone-compliance`: the profile (`legal_name`, `trading_name`,
  `entity_type`, `registration_country`, `company_number`, `company_registry` `{country, region, name}`,
  `tax_ids` (write-only, shown masked), `website`, `use_case`, `representative` (contact and birth data
  write-only), `addresses[]`, and `status` per country, type and action). Identity keys get `403 scope_denied`.
- `POST /v1/tenants/{tenant_id}/phone-compliance/documents` with `kind`, `filename`, `content_type`, `size`,
  `sha256`, `valid_until`: `201` with the document and a one-hour `upload_url`; `PUT` the bytes there. `GET`
  lists metadata; `GET`, `DELETE …/documents/{document_id}`. No route returns a document's bytes.
- `POST /v1/platform/phone-compliance/{profile_id}/export` (`platform:ops`) with `reason`: a single-use link to
  an encrypted export, audited.

### US registrations — `phone:read`, `phone:compliance`

`GET /v1/tenants/{tenant_id}/phone-registrations` returns `{configured, brand_status, campaign_status,
numbers_registered[], failure_reasons[], brand_entity, otp_status}`. For a US sole-proprietor brand,
`POST /v1/tenants/{tenant_id}/phone-registrations/{registration_id}/otp` with `{ "pin" }` and `POST …/otp/resend`
(at most 5 a day) are decisions reserved for people; `422 phone_registration_otp_invalid`.

### Voice connectors and routes — `phone:manage`

`POST`, `GET /v1/tenants/{tenant_id}/phone-connectors` (`{ "kind": "vapi", "region": "eu", "api_key" }`; the key
is never shown again), `DELETE …/{connector_id}`, `POST …/{connector_id}/rotate`. Routes change only through
`set_voice_route` requests.

### SMS — `phone:send`, `phone:read`, `quarantine:review`, `erasure:manage`

- `POST /v1/identities/{identity_id}/sms` (`Idempotency-Key` required except with `?dry_run=true`) with `from`,
  `to`, `text`, `kind`, `conversation_id`, `client_reference`: `202` with the SMS (`status: "queued"`). Refusals
  in order: `403 tenant_suspended`, `409 identity_paused`, `503 unavailable` (`phone_paused`), `404
  phone_number_not_found`, `409 phone_number_not_ready`, `403 policy_denied` (`destination_country`,
  `destination_untested`, `destination_not_supported`), `403 destination_blocked`, `403 recipient_opted_out`,
  `403 consent_required`, `403 a2p_not_registered`, `409 quiet_hours`, `429 daily_cap_reached`,
  `402 spend_cap_reached`. A dry run answers `200 { "would_send", "segments", "price_pence" }`.
- `POST …/sms/{sms_id}/resolve` (`phone:send`) with `outcome` `sent` or `not_sent` for an `uncertain` SMS.
- `GET /v1/identities/{identity_id}/sms`, `GET …/sms/{sms_id}`, `GET …/sms/{sms_id}/media/{n}`,
  `GET …/sms-conversations` (`phone:read`); codes in text appear as `[code]`.
- `GET /v1/identities/{identity_id}/sms/wait` (`phone:read`) with `number_id`, `from`, `account_id`, `kind`,
  `timeout` (1–60), `since`: `200 { "sms", "verification", "timed_out" }`.
- `POST …/sms/{sms_id}/release` (`quarantine:review`); `DELETE …/sms/{sms_id}` (`erasure:manage`).

### Calls — `phone:call`, `phone:read`

`POST /v1/identities/{identity_id}/calls` (`Idempotency-Key` required) with `from`, `to`, `purpose`, `assistant`:
`202` with the call, or with the `outbound_call` request under `phone.calls.outbound = approval`. `GET
/v1/identities/{identity_id}/calls`, `GET …/calls/{call_id}`, `GET …/phone-usage`; `DELETE …/calls/{call_id}`
(`erasure:manage`).

### Consent — `phone:read`, `phone:send`, `phone:manage`

`GET /v1/tenants/{tenant_id}/phone-consent?number=` returns the ledger entry (masked hint). `POST` with
`{ "number", "state", "purpose", "evidence" }`: `opted_out` needs `phone:send` (any key); `opted_in` needs
`phone:manage` and evidence.

### Port-outs — `phone:read`, `phone:approve`

`GET …/phone-numbers/{number_id}/port-outs`; `POST …/port-outs/{port_out_id}/authorize` and `…/reject` (a reason
code from the provider's list; `1001` needs text) are decisions reserved for people; `409 port_out_closed`.

### Money — `phone:read`

`GET /v1/tenants/{tenant_id}/phone-charges?month=` (charges, caps including the registration cap, holds) and
`GET /v1/partners/{partner_id}/phone-statement?month=` (partner keys and platform keys).

### Operations

`POST /v1/tenants/{tenant_id}/phone/reconcile` (`phone:manage`, once every 10 minutes); `POST
/v1/platform/phone/reconcile`, `POST /v1/platform/phone/probe` and `POST /v1/platform/phone-numbers/adopt`
(`platform:ops`); `POST /hooks/telnyx` (no key; Ed25519 signature over `telnyx-timestamp|raw body`).

## 2. Permission rows ("### Permissions")

| `phone:read` | The catalogue, setup status, budget, numbers, requests, SMS, calls, usage, registrations, consent, charges | every level |
| `phone:request` | Buy within the budget; search; create and cancel own purchase, port-in and release requests; onboarding drafts; shorten `expires_at` | every level |
| `phone:send` | Send SMS; resolve uncertain SMS; record an opt-out | every level |
| `phone:call` | Place calls or create `outbound_call` requests | every level |
| `phone:approve` | Approve and reject phone requests; decide port-outs (decisions reserved for people) | platform, partner, tenant |
| `phone:manage` | Number roles and moves; connectors; routes and emergency requests; opt-ins; tighten the budget; extend `expires_at`; tenant reconcile | platform, partner, tenant |
| `phone:compliance` | The compliance profile and documents; compliance, 10DLC and toll-free requests; the sole-proprietor code | platform, partner, tenant |

## 3. In "### Idempotency", add after the list of send operations

Phone routes that can cost money or move a number require it too: `POST …/phone-numbers` (one-call purchase),
`POST …/phone-requests` and its approve, `PUT …/phone-budget`, `POST …/sms` and `POST …/calls`.
