Where this goes: `docs/src/project/design/event-feed.md`. Rows in the table of "## 4. Who sees which events", rule 2. Source: [Phone numbers § 25](phone-numbers.md#25-events-and-audit).

| `sms.quarantined`, `sms.screened`, `sms.released` | `phone:read` and `quarantine:review` | own identity |
| every other `sms.*` type, `call.*` | `phone:read` | own identity |
| `phone_request.*`, `phone_number.*`, `phone_registration.*`, `phone_consent.changed`, `phone_spend.warning`, `phone_setup.status_changed`, `phone_budget.changed` | `phone:read` | own identity (events with an `identity_id`; tenant-wide phone events need a tenant-level key) |
| `phone_compliance.status_changed` | `phone:compliance` | never |

An SMS withheld for the vault (`vault_code`) is never appended to the log.
