Where this goes: `docs/src/project/design/notifications.md`. (1) A row in "## 1. Kinds"; (2) a sentence in "## 5. The emails". Source: [Vault § 6.5](vault.md#65-what-the-approver-sees), [§ 10.4](vault.md#104-health-and-hand-off).

## 1. Kind

| Vault needs a person | Vault grants and uses waiting for approval, items whose health is `needs_auth`, `needs_attention` or `needs_rotation`, and consumers waiting for activation | Owners and admins (default on), in the daily "needs a person" email and the console's Overview, as counts and item titles |

## 2. The emails

Vault rows show counts and item titles only; a grant's or use's `purpose`, written by an agent, is never in an
email, and nothing an agent wrote is rendered as a link ([Vault § 6.5](vault.md#65-what-the-approver-sees)).
