Where this goes: `docs/src/project/design/inbound.md`. (1) A row in the table of "## Quarantine decision" after rule 4a; (2) a paragraph at the end of "## Verification codes and unsolicited OTP (E5)"; (3) a sentence in "## The `wait` handler (E4)". Source: [Vault § 9.2](vault.md#92-email-codes).

## 1. Rule 4b

| 4b | A verification match with verdict `pass`, whose matched approved ledger entry has a linked `active` login item with `otp_channel.kind = "email"`, `otp_channel.address` equal to `delivered_to` and the sender's organisational domain in `otp_channel.sender_domains`; or, with `vault.codes_into_fills_only`, any verification match whatever its verdict, sender or entry, except receive-allowed senders ([Vault § 9.2](vault.md#92-email-codes)) | `hidden` | `vault_code` |

## 2. Paragraph

**Codes for the vault.** A message held by rule 4b is invisible to every agent surface, and unlike other hidden
mail its content is withheld from API keys whatever their permissions (`content_withheld: "vault_code"`; `raw`
answers `404`). It is never released and never counted in notifications. Its `verifications` row records
`account_id` and `vault_item_id`; `VaultTakeCode` consumes it once into an open `email_otp` phase. The message,
raw object and row are deleted 24 hours after `received_at`. The step-12 query for approved entries returns each
entry's linked items with an email channel (`IngestInput.approved_accounts[].vault_items`), so rule 4b reads
nothing more.

## 3. `wait`

With `vault.codes_into_fills_only: true`, `wait` with `kind=verification` answers `403 policy_denied` with
`details.reason = "codes_into_fills_only"`.
