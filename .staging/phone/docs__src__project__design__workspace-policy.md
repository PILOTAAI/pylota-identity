Where this goes: `docs/src/project/design/workspace-policy.md`. (1) Rows in "## 3. Decisions reserved for people"; (2) fields in "## 2. Classes and ceilings". Source: [Phone numbers § 5.4, § 5.7, § 16, § 19](phone-numbers.md#19-policy).

## 1. Rows for the decisions table in section 3

| Approve a phone request (purchase outside the budget, port-in, release, compliance submission, US registration, toll-free verification, emergency address, voice route, outbound call) | `POST …/phone-requests/{request_id}/approve` ([Phone numbers § 5.4](phone-numbers.md#54-approval)) |
| Decide a port-out | `POST …/port-outs/{port_out_id}/authorize`, `…/reject` ([Phone numbers § 16](phone-numbers.md#16-the-port-out-guard)) |
| Set or loosen the standing purchase budget | `PUT /v1/tenants/{tenant_id}/phone-budget` ([Phone numbers § 5.7](phone-numbers.md#57-the-standing-purchase-budget)) |

For these three, the tenant opt-in is `phone.key_approval` (not `quarantine.key_release`): a platform key or the
tenant's partner key may decide with `on_behalf_of`; tenant and identity keys never.

## 2. Classes

Guard fields: `phone.auto_approve` (an object; tightening any field is allowed to `phone:manage`),
`phone.calls.outbound`, `phone.recording`, `phone.disclosure.calls`, `phone.disclosure.first_sms`,
`phone.verification.redact`. Platform or own-partner: `phone.allowed` (to turn on), `phone.key_approval`.
Lower-only: every other numeric and list field of `phone.*`, including `phone.spend.registration_cap_pence`.
