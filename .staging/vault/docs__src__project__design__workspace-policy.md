Where this goes: `docs/src/project/design/workspace-policy.md`. (1) Rows in the table of "## 3. Decisions reserved for people"; (2) a paragraph after the drafts paragraph; (3) the vault fields in "## 2. Classes and ceilings". Source: [Vault](vault.md).

## 1. Rows

| Approve a vault grant or use, or loosen a vault item's approval level | `POST /v1/vault/grants/{grant_id}/approve`, `POST /v1/vault/uses/{use_id}/approve`, `PATCH /v1/vault/items/{item_id}` ([Vault § 5](vault.md#5-grants), [§ 6.4](vault.md#64-approving-denying-and-cancelling)) |
| Activate or resume a vault consumer | `POST /v1/vault/consumers/{consumer_id}/activate` and `…/resume` ([Vault § 8.2](vault.md#82-registration-activation-and-suspension)) |

## 2. Paragraph

For vault approvals the tenant's opt-in is `vault.key_approval`: `key_may_decide(Decision::ApproveVault, …)` reads
that field and admits only a platform key or the tenant's own partner key, which must name the person with
`on_behalf_of`. `quarantine.key_release` and `drafts.key_approval` do not open it. `Decision::ActivateConsumer`
has no tenant opt-in at all: a platform key decides, or a signed-in owner or admin for a tenant-level self-hosted
consumer, or any `vault:admin` key with `PM_CONSOLE=off`. Identity keys can never hold `vault:approve` or
`vault:admin`, so none of these reaches an agent.

## 3. Classes

`vault.allowed` is platform-only; `vault.key_approval` is platform-or-own-partner; `vault.max_items`,
`vault.session_max_ttl_days` and `vault.approval_deadline_seconds` are lower-only; `vault.min_approval` (order
`grant` < `per_run` < `per_use`) and `vault.codes_into_fills_only` (`true` stricter) are guard fields
([Configuration › Who may change a field](../../reference/configuration.md#who-may-change-a-field)).
