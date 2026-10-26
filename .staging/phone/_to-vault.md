For the vault writer and the coordinator, not for pasting: what the phone design (`docs/src/project/design/phone-numbers.md`, ADR 0020) takes from the vault, as reconciled on 2026-10-25 against `docs/src/project/design/vault.md` § 9.3 and § 13.2.

# Phone design → vault interface

The vault design already offers what the phone design needs. The phone design adopts the vault's
interfaces as written and asks for no new operation. This page records how each is used, and the two
points the vault writer settled on 2026-10-26.

## V-1 Port-out PIN: managed secret (vault § 13.2)

Used exactly as offered:

```rust
put_managed(ctx, tenant_id, Some(identity_id), owner_ref = number_id /* pnm_… */,
            ManagedPurpose::PortOutPin, value: Zeroizing<String>) -> Result<ItemId>
delete_managed(ctx, tenant_id, owner_ref = number_id, ManagedPurpose::PortOutPin) -> Result<()>
```

- **Who calls.** Only the phone service: the configure step at activation and adoption
  (phone-numbers § 8, "Ownership stamp"), the port-out guard after a rejected port-out (§ 16), the 72-hour
  rotation after a reveal (§ 16), and the reconciler when the provider's PIN is missing or differs (§ 17).
  `delete_managed` at release (§ 7.4) and when a port-out completes (§ 16).
- **Never `use_managed`.** The phone service never reads a PIN back. Every change generates a new 8-digit
  value from `platform::Rng`, sets it at Telnyx (`PATCH /phone_numbers/{id}` with `external_pin`), reads the
  number back, then calls `put_managed` with the new value, which replaces the item (one live managed secret
  per `owner_ref` and purpose). This answers vault § 13.2's open point about partner custody: sealing needs
  only the public key, so nothing the phone service does needs the partner's unlock key.
- **Ordering and failure.** The provider is set first, then the vault. If `put_managed` fails, the number
  stays `configuring` and the step is retried; after an hour `phone_pin_missing` is raised. A provider PIN
  whose vault copy was never written is treated as unknown and rotated by the next successful run.
- **Reveal.** Owners and admins reveal the PIN in the console (vault § 13.1). In partner custody, where the
  console cannot reveal, the phone console's **Port PIN** action rotates the PIN and shows the new value once
  to the signed-in person (`Cache-Control: no-store`, audit `phone_number.pin_shown`); this is a phone-side
  action, not a vault reveal. **Settled (2026-10-26):** the vault owns it. In partner custody the console offers "Show a new value"
  (`rotate_and_show_managed`) for managed secrets: the vault calls the phone service's registered callback for
  `ManagedPurpose::PortOutPin`, which sets a new `external_pin` at Telnyx and reads it back, then seals it with
  `put_managed` and shows it once (vault § 13.2). The phone console's **Port PIN** action calls that, so there
  is one reveal surface; audit `item.revealed` with `details.rotated = true`.
- **Erasure.** Tenant erasure's `phone_release` step deletes each number's PIN with `delete_managed` before
  the vault's own tenant step runs; the vault's step deletes any left (for example after a crash). The vault
  receipt counts managed secrets; the phone receipt does not count them again.
- **Listing.** Managed PINs appear in vault listings with `managed_by: "phone"` and their `owner_ref`; the
  phone design shows the PIN's presence (`port_pin: "set"`), never its value, on the number object.

## V-2 SMS codes into fills (vault § 9.3)

The phone design implements the three needs the vault lists:

| Vault need | Phone design |
|---|---|
| A code source, consuming once | `phone::codes::SmsCodes` implements `SmsCodeSource::take(tenant_id, number_id, senders, since_ms, use_id) -> Option<OneTimeCode>` by `MailboxRequest::VaultTakeSmsCode { number_id, senders, since_ms, use_id }` to the mailbox of the number's identity: one transaction selects the oldest `sms_verifications` row of the number with `expires_at > now`, `vault_consumed_at IS NULL`, `received_at >= since_ms` and a sender in `senders` (exact match: E.164, short code digits, or the alphanumeric sender folded to upper case), sets `vault_consumed_at` and `vault_use_id`, and returns `{kind: "code", value}`. Spent even if the redemption fails afterwards (fail closed) |
| Withholding | Quarantine rule 2b (phone-numbers § 10.2, § 11.4): an inbound SMS to a number named by an `active` login item with `otp_channel.kind = "phone"`, from one of its `senders`, with a verification match, is stored `hidden` with `quarantine_reason = 'vault_code'`, withheld from every agent-facing surface and from API keys' reads, never released, and deleted with its code row 24 hours after arrival. The match uses the email rule's patterns (`core::classify::find_verification`) with the link rule off: a link in an SMS is never a verification object |
| Strict mode | `vault.codes_into_fills_only` withholds every SMS with a verification match on the tenant's numbers, except receive-allowed senders; `sms/wait` with `kind=verification` then answers `403 policy_denied`, `details.reason = "codes_into_fills_only"` |
| Erasure | Every phone erasure scope (SMS, conversation, counterparty, identity, tenant) deletes withheld SMS and their code rows, counted in `sms_deleted` |

**Facts the phone consumer needs from the vault** (one read, only for an SMS with a verification match):
the identity's `active` login items with `otp_channel.kind = "phone"`, as `{item_id, number_id, senders}`.
Proposed shape, mirroring the email case (`IngestInput.approved_accounts[].vault_items`):
`vault::phone_channels(tenant_id, identity_id) -> Vec<PhoneChannel { item_id, number_id, senders }>`, a D1 read
of `vault_items` with no decryption. **Settled (2026-10-26):** it lives in the vault crate,
`crates/worker/src/vault/channels.rs`, as `vault::phone_channels`, beside the email side's facts, so the rule
lives in one place (vault § 9.3).

**Constraints the phone side relies on:** the vault calls `take` only for a use created at most 10 minutes
earlier and only inside an open `sms_otp` phase; `senders` has at most 10 entries; `number_id` is a live
number of the use's identity (the phone side re-checks it and returns `None` otherwise).

## V-3 Nothing else

The phone design keeps its other secrets as sealed columns, as SMTP relay credentials are: per-tenant SIP
credentials, Vapi keys and server secrets, KYC data keys, sealed profile fields and port-in losing-carrier
details (phone-numbers § 23). They are used by the Worker itself on every provisioning run, so the vault's
grant and ticket model does not fit them. If the vault later offers Worker-internal secrets beyond
`managed_secret`, moving these is a separate change.
