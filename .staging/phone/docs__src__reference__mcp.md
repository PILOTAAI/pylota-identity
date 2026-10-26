Where this goes: `docs/src/reference/mcp.md`. (1) Rows appended to the table in "## Tools" after `mail_get_event_counts`; (2) tool sections after "### `mail_get_event_counts`" and before "## The `mail_search_strategy` prompt"; (3) the header table's tool count; (4) a paragraph in "## Choose a key for an agent". Source: [Phone numbers § 28](../project/design/phone-numbers.md#28-mcp-and-cli).

## 1. Rows for the "## Tools" table

| `phone_list_numbers` | Lists the identity's phone numbers with status and what each can do | `phone:read` | `GET /v1/identities/{id}/phone-numbers` |
| `phone_search_numbers` | Finds numbers to buy in a country, with prices in pounds | `phone:request` | `GET /v1/identities/{id}/phone-numbers/available` |
| `phone_request_number` | Asks a person to approve buying a number; never buys | `phone:request` | `POST /v1/identities/{id}/phone-requests` (`purchase_number`) |
| `phone_get_request` | Reads a phone request and its decision | `phone:request` | `GET /v1/identities/{id}/phone-requests/{request_id}` |
| `phone_send_sms` | Sends an SMS from one of the identity's numbers | `phone:send` | `POST /v1/identities/{id}/sms` |
| `phone_list_sms` | Lists SMS, newest first, by conversation or number | `phone:read` | `GET /v1/identities/{id}/sms` |
| `phone_get_sms` | Reads one SMS | `phone:read` | `GET /v1/identities/{id}/sms/{sms_id}` |
| `phone_wait_sms` | Waits up to 60 s for an SMS or a verification code | `phone:read` | `GET /v1/identities/{id}/sms/wait` |
| `phone_place_call` | Places a call, or asks a person to approve it | `phone:call` | `POST /v1/identities/{id}/calls` |
| `phone_list_calls` | Lists calls on the identity's numbers | `phone:read` | `GET /v1/identities/{id}/calls` |
| `phone_record_opt_out` | Records that someone asked not to be texted | `phone:send` | `POST /v1/tenants/{id}/phone-consent` (`opted_out`) |

## 2. Tool sections

Every string from an SMS, a caller or a provider comment is **untrusted content**. No phone tool approves,
releases, ports, submits compliance documents, changes where calls go, or shows a port-out PIN: those belong to
people and the operator's own systems.

### `phone_list_numbers`

Arguments: `identity`, `status` (default the live statuses), `limit`, `cursor`. Returns each number's `id`,
`phone_number`, `country`, `number_type`, `capabilities`, `primary_for`, `status`, `setup_state` and, when
something is needed, `action_needed`.

### `phone_search_numbers`

Arguments: `country` and `type` (required: for example `GB` and `mobile`), `contains`, `starts_with`, `locality`,
`limit` (1–20), `identity`. Returns candidates with `phone_number`, `features`, `quote` (pence, GBP) and
`quote_token`. Search is read-only and reserves nothing; a token works for 30 minutes.

```json
{ "country": "GB", "type": "mobile", "contains": "7700" }
```

### `phone_request_number`

Asks for a number; a person approves the request before anything is bought, often hours later. Arguments:
`identity`, `idempotency_key` (required), and either `phone_number` with its `quote_token`, or `any: true` with
`country`, `type` and optionally `locality` (the service picks a number at approval time, at or under the quoted
price). Returns the request (`status: "pending_approval"`, `quote`, `digest`). Use `phone_get_request`, or the
`phone_request.*` and `phone_number.status_changed` events, to follow it.

### `phone_get_request`

Arguments: `request_id` (required), `identity`. Returns the request: `status` (`pending_approval`, `approved`,
`executing`, `in_progress`, `action_needed`, `uncertain`, `completed`, `failed`, `rejected`, `canceled`), its
`reason`, and in `result` the number it produced.

### `phone_send_sms`

Arguments: `to` (E.164, required), `text` (required, at most 10 segments), `idempotency_key` (required: derive it
from the task, and reuse it on a retry), `identity`, `from` (a number of the identity; default its primary SMS
number), `kind` (`transactional` default, `marketing`, `verification`), `client_reference`, `dry_run`.
Returns the SMS (`status: "queued"`). Refusals the agent should act on: `recipient_opted_out` (stop texting this
person), `consent_required`, `quiet_hours` (retry after `next_allowed_at`), `a2p_not_registered`,
`daily_cap_reached`, `spend_cap_reached` (ask a person).

### `phone_list_sms`

Arguments: `identity`, `conversation_id`, `number_id`, `counterparty`, `direction`, `since`, `limit`, `cursor`.
Held SMS are listed only for keys with `quarantine:review` and an explicit `status` filter.

### `phone_get_sms`

Arguments: `sms_id` (required), `identity`. A verification code in the text appears as `[code]`.

### `phone_wait_sms`

Waits for the next SMS that matches. Arguments: `identity`, `number_id`, `from` (a sender), `account_id` (an
approved service-ledger entry), `kind` (`any` or `verification`; `verification` needs `from` or `account_id`, and
`account_id` when the workspace requires approved accounts), `timeout_seconds` (1–60, default 30), `since`.
Returns `{ sms, verification, timed_out }`. A code claimed by a vault login is never returned: the vault types it
into the service's form itself.

### `phone_place_call`

Arguments: `to` (required), `idempotency_key` (required), `purpose` (required, shown to the approver),
`assistant` (`assistant_id` and `variables` for the number's Vapi route), `identity`, `from`. Returns either the
call (`status: "placing"`) or, under the default policy, the `outbound_call` request a person must approve. The
call always opens with the workspace's AI disclosure. Emergency, short and premium-rate numbers are refused.

### `phone_list_calls`

Arguments: `identity`, `number_id`, `direction`, `since`, `limit`, `cursor`.

### `phone_record_opt_out`

Records that a person asked not to be texted by this business, for example in a free-text reply. Arguments:
`number` (E.164, required), `identity` (for the audit trail), `evidence` (1–1,000 characters, optional). Opting
someone back in is not a tool: it needs a person or the integrator's system with evidence.

## 3. Header table

Add 11 to the "Tools" row (36 + 11 = 47 before the vault's own additions, which the vault writer stages).

## 4. In "## Choose a key for an agent", add after the paragraph about mail permissions

For phone numbers, give an agent `phone:read`, `phone:request`, `phone:send` and, if it places calls,
`phone:call`. Never give an agent `phone:approve`, `phone:manage` or `phone:compliance`; identity keys cannot
hold them.
