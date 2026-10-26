Where this goes: `docs/src/project/design/privacy.md`. Insertions in four sections. Source: [Vault § 16](vault.md#16-retention-and-erasure).

## 1. In "## 2. Data inventory", add these rows

| Vault items, versions and session objects | Credentials and sessions of the tenant's and its agents' third-party accounts; usernames are personal data | D1 (`vault_items`, `vault_item_versions`), R2 `t/{tenant_id}/vault/` | Until deleted; session versions until their TTL; `previous` versions for their overlap |
| Vault grants and uses | Who may use what, when, for which run | D1 | 90 days after they end |
| Vault audit | IDs, sites (registrable domains), times, outcomes; no values, usernames, titles, purposes or URLs | D1 `vault_audit`, `vault_audit_anchors` | 400 days by age, never by count |
| Hidden `vault_code` mail and its `verifications` rows | One-time codes | Mailbox | 24 hours |

## 2. In "### 5.3 Global retention job", add a step `vault`

Terminal grants and uses after 90 days (ticket hashes and sealed keys were already cleared when they ended);
`vault_audit` rows older than 400 days, writing the last deleted row's `seq` and `row_hash` to
`vault_audit_anchors` first; revoked consumers after 400 days; never a row by count. The every-minute vault cron
and the daily vault sweep handle overlaps, nonces, expired sessions and orphan R2 objects.

## 3. In "## 6. Erasure"

- **6.5 Identity scope**: in step `scrub_control_plane`, before `service_accounts`: the identity's identity-scoped
  vault items are crypto-shredded (versions, grants, uses closed, R2 objects by key); its grants on tenant-scoped
  items deleted and their uses closed; vault audit rows kept with IDs only and `run_id` cleared.
- **6.6 Tenant scope**: step `stop_routing` closes open vault uses; step `delete_d1_rows` deletes, in order,
  `vault_custody_slots`, `vault_keys` (the crypto-shred), `vault_uses`, `vault_grants`, `vault_item_versions`,
  `vault_items`, tenant-level `vault_consumers`, `vault_audit` and `vault_audit_anchors` except the
  `vault.revoke_all` row the erasure writes; step `sweep_r2` sweeps `t/{tenant_id}/vault/`.
- **6.7 Probes**: no `vault_items`, `vault_item_versions`, `vault_keys` or `vault_custody_slots` row of the scope
  remains, and `t/{tenant_id}/vault/` lists empty for a tenant scope.
- Counterparty, message and thread scopes do not touch vault rows; a hidden `vault_code` message is erased with
  its message like any other.

## 4. In "## 10. Receipt", add the fields

`vault_items_deleted`, `vault_versions_shredded`, `vault_keys_shredded` (tenant scope), `vault_uses_deleted`,
`vault_audit_deleted`.

## 5. In "## 11. What remains after deletion"

Add: "A D1 Time Travel restore within 30 days can bring back vault rows; the erasure re-application after every
restore deletes them again. In partner custody, restored rows cannot be opened once the partner has discarded its
unlock keys. Deleting a session item does not sign the session out at the site."

## 6. In "## 9. Subject-access export"

Add: "Vault items appear in `vault.json` as metadata only (kind, title, origins, health, dates, the ledger entry);
no value and no ciphertext is ever exported."
