Where this goes: `docs/src/project/design/privacy.md`. (1) Rows in "## 2. Data inventory"; (2) bullets in "### 6.4", "### 6.5" and "### 6.6"; (3) fields in "## 10. Receipt"; (4) rows in "## 11. What remains after deletion"; (5) files in "### 9.2 ZIP layout". Source: [Phone numbers § 24](phone-numbers.md#24-privacy).

## 1. Data inventory rows

Copy the table of [Phone numbers § 24](phone-numbers.md#24-privacy) into section 2 as rows, with the columns
mapped: store; personal data; purpose (provider events: process and reconcile; SMS and calls: the identity's
messages; consent: honour opt-outs; compliance: register numbers in the tenant's name; onboarding drafts:
prepare a registration a person submits; charges: billing); retention; deletion mechanism.

## 2. Erasure scopes

- **Counterparty** (6.4): a counterparty may be an E.164 number; its SMS, media and calls in every identity of the
  tenant are deleted, and its consent entry keeps the keyed hash only.
- **Identity** (6.5): `DELETE /v1/identities/{id}` is refused while the identity holds a live number
  (`409 identity_has_phone_numbers`); the mailbox wipe covers `sms_*` and `calls`.
- **Tenant** (6.6): a step `phone_release` right after routing stops: cancel open requests (blocked ones
  included), release every live number, cancel cancellable porting orders, deactivate 10DLC campaigns, delete
  the tenant's provider resources and connectors, delete onboarding drafts, then the compliance data, except
  documents in their KYC retention period, which are listed in the receipt.

## 3. Receipt fields

`sms_deleted`, `sms_media_deleted`, `calls_deleted`, `phone_numbers_released` (with, for platform and partner
keys, `{provider_number_id, provider_order_id, e164, vapi_phone_number_id}` per number),
`phone_documents_deleted`, `phone_documents_retained` (with `retain_until`), `phone_consent_entries_deleted`,
`phone_onboarding_drafts_deleted` and `provider_deletions` (`attempted`, `failed`).

## 4. What remains after deletion

| Telnyx message and call detail records | Numbers, times, costs | Telnyx's own retention; content not kept when the profile's redaction is on |
| Telnyx KYC copies after the last number | Documents and addresses | Deleted by the executor; a copy still linked by the provider is retried daily for 30 days, then reported |
| Vapi call data of `vapi` routes | Call content and transcripts | The tenant's own Vapi account |
| KYC documents in their retention period | Identity documents | Until `retain_until`, under the hold reason `kyc_retention` |

## 5. ZIP layout

`sms.json` and `calls.json` (the identity's, or a counterparty's across identities).
