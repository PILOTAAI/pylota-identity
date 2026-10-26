Where this goes: `docs/src/project/design/service-accounts.md`. (1) Two fields in "## 2. The entry"; (2) a paragraph at the end of "## 5. The verification gate". Source: [Phone numbers § 11.2](phone-numbers.md#112-the-gate).

## 1. Fields

| `phone_number_id` | Optional: the identity's number a service texts codes to (`pnm_`). An entry naming no number matches SMS only when the identity has one live SMS number |
| `sms_senders` | Optional, up to 10 senders (E.164, short codes, alphanumeric sender IDs) the service's codes come from |

## 2. SMS

Codes in SMS pass the same gate with a stricter match, because SMS senders cannot be authenticated: the entry
must name the receiving number (or the identity must have one SMS number), an `sms/wait` naming the entry must
have been registered within 30 minutes, and a listed sender must match. Codes that match no entry are
quarantined `account_unapproved`. Codes on a number a vault login item names are withheld for the vault
([Phone numbers § 11](phone-numbers.md#11-verification-codes-over-sms)).
