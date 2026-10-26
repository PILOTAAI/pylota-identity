Where this goes: `docs/src/project/design/observability.md`. (1) Rows in "### 3.2 Catalogue"; (2) rows in "### 5.3 Alert list"; (3) a row in "### 5.6 Automatic containment"; (4) a runbook "### Vault incident" at the end of "## 9. Runbooks". Source: [Vault](vault.md).

## 1. Metrics

| `vault_uses_total` | counter | operation, outcome: `approved_at_once`, `approval_requested`, `refused` | Vault use routes |
| `vault_redemptions_total` | counter | phase, result: `ok`, `refused`, `expired`, `spent`, `unauthenticated` | Consumer redeem route |
| `vault_redeem_ms` | observation | phase | Consumer redeem route (excluding the code long poll) |
| `vault_consumer_reports_total` | counter | outcome | Consumer report route |
| `vault_value_exposed_total` | counter | source: `report`, `unscannable` | Consumer report route |
| `vault_placeholder_misplaced_total` | counter | – | Consumer report route |
| `vault_unlock_failures_total` | counter | – | Unwrapping routes |
| `vault_audit_append_failures_total` | counter | – | Every vault write |
| `vault_decrypt_failed_total` | counter | where: `dek`, `field`, `ticket` | Mint, redeem, reveal, sign |
| `vault_consumer_auth_failures_total` | counter | – | `auth::consumer::verify` |

## 2. Alerts

| `vault_value_exposed` | B | `vault_value_exposed_total` > 0 for a tenant outside test tenants | page | [Vault incident](#vault-incident) |
| `vault_audit_append_failed` | B | `vault_audit_append_failures_total` > 0 | page | [Vault incident](#vault-incident) |
| `vault_decrypt_failed` | B | `vault_decrypt_failed_total` > 0 (a moved or corrupted ciphertext, or a wrong key) | page | [Vault incident](#vault-incident) |
| `vault_placeholder_misplaced` | B | The first `placeholder_misplaced` report per tenant and UTC day | ticket | [Vault incident](#vault-incident) |
| `vault_unlock_failures` | A | `vault_unlock_failures_total` > 20 in 15 minutes | ticket | [Vault incident](#vault-incident) |
| `vault_consumer_auth_spike` | A | Anomaly on `vault_consumer_auth_failures_total`, 15-minute window, minimum 50 | ticket | [Vault incident](#vault-incident) |

## 3. Automatic containment

| `vault_redeem` | A `vault_decrypt_failed` alert, or three `vault_value_exposed` from one consumer within an hour | The ops switch `vault_redeem` moves to `off`: redemption, write-back and minting refuse; reads and approvals continue. The operator turns it back on after the runbook |

## 4. Runbook

### Vault incident

([Vault § 15](vault.md#15-kill-switches-and-incident-response))

1. **Contain.** For a suspected consumer leak, suspend the consumer (`idvy vault consumers suspend`). For a
   tenant-wide suspicion, `idvy vault revoke-all --tenant T --shred-sessions`. For a platform-wide one,
   `idvy ops switch vault_redeem off`; `PM_VAULT=off` only as a last resort (it stops every vault route).
2. **Scope.** Read the tenant's audit from the suspected time (`idvy vault audit --tenant T --verify`): which
   `ticket.redeemed` rows, by which consumer, for which items. A chain that fails verification is itself the
   incident: stop and preserve D1 (Time Travel bookmark) before anything else.
3. **Rotate.** Every item redeemed by a suspected consumer, or reported `value_exposed`, is rotated at its
   service first, then in the vault. Sessions: shred, and ask the person to sign out other sessions at the site.
4. **Keys.** A leaked consumer key: rotate the consumer's keys. A leaked orchestrator key: revoke it. Master key
   and D1 together: rotate the master key and treat every operator-custody value as exposed.
5. **Notify.** Tell the tenant's owners within 24 hours with the items and times concerned; counsel decides on
   regulator notification ([Vault § 23](vault.md#23-questions-for-counsel)).
6. **Reopen** by turning the switch back on, then watch `vault_redemptions_total{result=refused}` and the
   consumer reports for a day.
