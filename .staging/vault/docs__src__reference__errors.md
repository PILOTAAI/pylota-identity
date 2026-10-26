Where this goes: `docs/src/reference/errors.md`, "## Catalogue". Rows go into the existing tables named before each block, in HTTP order within the table; the last block amends existing rows. Twenty-two new codes. Source: [Vault](../project/design/vault.md).

## Into "### Authentication and access" (after the phone rows, or after the `invalid_signature` row)

| 401 | <a id="code-consumer_unauthenticated"></a>`consumer_unauthenticated` | no | A request to a vault consumer route (`/v1/vault/consumer/*`) whose RFC 9421 signature is missing, malformed, stale, made with an unknown key, over the wrong components or tag, whose `Content-Digest` does not match the body, whose nonce was used before, or that carries an `Authorization` header. The body is the same whatever the cause ([Vault § 8.4](../project/design/vault.md#84-authenticating-a-consumer)). Fix the consumer's clock, keys or signing code |
| 403 | <a id="code-vault_not_granted"></a>`vault_not_granted` | no | The identity has no active grant covering the item, operation and origin, or the grant's ledger entry is no longer approved. `details.reason`: `no_grant`, `operation_not_granted`, `origin_not_granted`, `grant_expired`, `max_uses_reached` or `ledger_not_approved`. Ask for a grant (`vault_request_grant`) |
| 403 | <a id="code-vault_refused"></a>`vault_refused` | no | The vault refused to use an item now. At use creation: `item_archived`, `item_needs_auth`, `item_needs_rotation`, `session_expired`, `session_in_use`, `consumer_kind_not_allowed`. At redemption, which also closes the use: `run_mismatch`, `top_level_origin_mismatch`, `frame_origin_mismatch`, `trigger_not_allowed`, `form_kind_not_allowed`, `signup_after_first_use`, `recording_on`, `logging_on`, `captcha_solving_on`, `extensions_present`, `hosting_mismatch`, `mode_not_declared` ([Vault § 8.6](../project/design/vault.md#86-redemption-checks)) |
| 403 | <a id="code-consumer_not_active"></a>`consumer_not_active` | no | A vault consumer that is `pending_activation`, `suspended` or `revoked`, or whose partner is suspended. A person or the platform activates it after review |
| 403 | <a id="code-vault_unlock_required"></a>`vault_unlock_required` | no | The tenant's vault is in partner custody and the route needs the partner's unlock key in `X-Pylota-Identity-Vault-Unlock`; also a console reveal in partner custody |
| 403 | <a id="code-vault_unlock_invalid"></a>`vault_unlock_invalid` | no | The unlock key opens no custody slot. Counted and rate-limited |

## Into "### Resources and state"

Add to the combined `404` row's list of codes: <a id="code-vault_item_not_found"></a>`vault_item_not_found`,
<a id="code-vault_grant_not_found"></a>`vault_grant_not_found`, <a id="code-vault_use_not_found"></a>`vault_use_not_found`,
<a id="code-vault_consumer_not_found"></a>`vault_consumer_not_found`,
<a id="code-vault_ticket_not_found"></a>`vault_ticket_not_found`. A ticket bound to another consumer, or of a
tenant outside the consumer's reach, answers `vault_ticket_not_found`, the same as an unknown one.

Then, after the phone `409` rows (or after the `signup_human_unreachable` row):

| 409 | <a id="code-vault_grant_exists"></a>`vault_grant_exists` | no | A pending or active grant of the same identity and item already covers the operations and origins asked (`details.grant_id`, `details.status`) |
| 409 | <a id="code-vault_not_pending"></a>`vault_not_pending` | no | Approve or deny of a grant or use that is not `pending_approval`, or that passed its deadline (`details.status`) |
| 409 | <a id="code-vault_phase_spent"></a>`vault_phase_spent` | no | The ticket's phase was already redeemed, or the use already wrote back. A phase is spent when the redemption commits, even if the answer was lost: create a new use |
| 409 | <a id="code-vault_totp_step_used"></a>`vault_totp_step_used` | yes | Another use of the same login took this TOTP time step. `details.retry_after` is the seconds to the next step; the phase stays open |

And after the `upload_expired` row:

| 410 | <a id="code-vault_ticket_expired"></a>`vault_ticket_expired` | no | The use is closed or expired. `details.reason` is its `close_reason` (`not_redeemed`, `window_elapsed`, `item_rotated`, `grant_revoked`, `ledger_closed`, `kill_switch`, `refused`, `value_exposed`, …). Create a new use |

## Into "### Policy and limits"

| 422 | <a id="code-vault_disabled"></a>`vault_disabled` | no | The vault is off on this deployment (`PM_VAULT`), redemption is switched off (`vault_redeem`), or the tenant's `vault.allowed` is `false` |
| 422 | <a id="code-vault_kind_refused"></a>`vault_kind_refused` | no | The vault never stores this: `details.reason` `payment_card`, `payment_card_detected` (a value that is a card number), `bank_account`, `passkey`, `refused_domain` or `confusable_origin`. Keep payment cards with your payment provider |
| 422 | <a id="code-vault_item_limit_reached"></a>`vault_item_limit_reached` | no | 100 items for the identity, or the tenant's `vault.max_items` (`details.limit`) |
| 422 | <a id="code-vault_grant_limit_reached"></a>`vault_grant_limit_reached` | no | 20 pending grant requests for the identity, or 50 grants in a non-terminal state for the item (`details.limit`) |
| 422 | <a id="code-vault_consumer_limit_reached"></a>`vault_consumer_limit_reached` | no | 10 platform-level, 10 per partner or 5 per tenant consumers that are not revoked (`details.limit`) |
| 422 | <a id="code-vault_session_too_large"></a>`vault_session_too_large` | no | A session state over 1,048,576 bytes, 500 cookies or 10 origins after filtering (`details.limit`) |

## Amendments to existing rows

- `permission_denied`: add "A vault grant or use approved by an API key where people decide gets it with
  `details.reason = "person_required"`, and approved by the key that asked for it with `"self_approval"`; an
  identity key minted with `vault:use`, `vault:approve` or `vault:admin` is refused earlier, with
  `400 invalid_request` (`permission_not_allowed_for_level`)."
- `policy_denied`: add "`wait` with `kind=verification` while the tenant's `vault.codes_into_fills_only` is
  `true` (`details.reason = "codes_into_fills_only"`); any vault route for a Free-plan workspace on a
  Stripe deployment (`details.reason = "plan_required"`)."
- `invalid_request`: add the vault reasons `ledger_entry_not_approved`, `origin_not_in_ledger`,
  `private_key_supplied`, `contract_unsupported`, `managed_item`, `sms_not_available`, `unlock_not_accepted`.
- `rate_limited`: add "`details.bucket = "vault_pending"` when a tenant has 100 vault uses waiting for approval".
