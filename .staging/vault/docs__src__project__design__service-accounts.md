Where this goes: `docs/src/project/design/service-accounts.md`. (1) Two fields in "## 2. The entry"; (2) a paragraph in "## 4. Approval"; (3) a sentence in "## 7. Retention and erasure"; (4) a row in "## 9. MCP and CLI". Source: [Vault § 3.3](vault.md#33-ledger-binding), [§ 3.5](vault.md#35-a-login-born-from-a-ledger-approval).

## 1. Fields

| `login_domains` | 0–5 organisational domains, besides `service_domain`, where the account signs in (for a service whose sign-in page is on another domain). Shown apart on the approve page, because approving them lets a vault fill happen there. Stored in `login_domains_json` |
| `vault_login` | Request only, optional: `{ origins, password: { generate } , email_otp, approval }`. Not stored on the entry; kept with the request until the decision |

## 2. Approval

When an entry with `vault_login` is approved, the same D1 batch creates the identity-scoped vault login item
(generated password; `otp_channel` set to the entry's address and `sender_domains` when `email_otp` is true) and
an active grant to the requesting identity for `fill` on those origins, expiring after 90 days. The approve page
says so: "Approving also creates a login this agent can use to sign in at …". A rejected or expired entry creates
nothing.

## 3. Retention and erasure

Closing or deleting an entry archives every vault item linked to it, revokes every grant that relied on it and
closes every open use (`ledger_closed`), in the batch of the closure ([Vault § 3.3](vault.md#33-ledger-binding)).

## 4. MCP and CLI

`mail_request_account` and `idvy accounts request` accept `--login-domain` (repeatable) and the `vault_login`
object.
