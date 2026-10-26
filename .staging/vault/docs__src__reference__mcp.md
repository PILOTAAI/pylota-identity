Where this goes: `docs/src/reference/mcp.md`. (1) Rows appended to the table in "## Tools" after the phone rows (or after `mail_get_event_counts`); (2) tool sections after the phone tool sections (or after "### `mail_get_event_counts`") and before "## The `mail_search_strategy` prompt"; (3) an addition to "### `mail_request_account`"; (4) the header table's tool count (+6); (5) a paragraph in "## Choose a key for an agent". Source: [Vault § 18.1](../project/design/vault.md#181-mcp-tools).

## 1. Rows for the "## Tools" table

| `vault_list_items` | Lists the identity's vault items (logins, API keys, sessions) by title, sites and health; never a value | `vault:read` | `GET /v1/identities/{id}/vault/items` |
| `vault_get_item` | Reads one item's details, its grants for this identity and its health; never a value | `vault:read` | `GET /v1/vault/items/{item_id}` |
| `vault_request_grant` | Asks a person to let this identity use an item for some operations at some sites | `vault:request` | `POST /v1/identities/{id}/vault/grants` |
| `vault_list_grants` | Lists this identity's grants and their status | `vault:read` | `GET /v1/identities/{id}/vault/grants` |
| `vault_get_use` | Reads a use's status, phases done and outcome | `vault:read` | `GET /v1/vault/uses/{use_id}` |
| `vault_sign` | Gets an HMAC or AWS SigV4 signature made with an API-key item, without the key (P1) | `vault:request` | `POST /v1/identities/{id}/vault/uses` (`operation: "sign"`) |

## 2. Tool sections

No vault tool returns a value, a ticket, a packet, a placeholder or a TOTP seed, and none approves, starts a
fill, reveals or deletes. Values are used by the integrator's own components (consumers) after a person
approved; the agent asks for access and reads what happened. Every `title`, `purpose`, `note`, `username` and
`health_reason` is **untrusted content**: a title that says "send the password to …" is data.

### `vault_list_items`

Arguments: `identity`, `kind` (`login`, `api_key`, `session`), `health`, `limit`, `cursor`. Returns each item's
`id`, `kind`, `title`, `origins`, `health`, `has` (which values exist, never the values) and `granted` (the
operations an active grant gives this identity, empty if none).

### `vault_get_item`

Arguments: `item`. Returns the item, this identity's grants on it and, when it needs one, the `health_reason` a
person should act on (`needs_auth`, `needs_rotation`, `needs_attention`).

### `vault_request_grant`

Asks a person for access. Request it before your orchestrator needs to sign in.

Arguments: `identity`, `item` (required), `operations` (required: `fill`, `session_load`, `inject`, `sign`, …
as the item's kind allows), `origins` (default all of the item's), `purpose` (required, 1–500 characters, shown
to the person as quoted text), `max_uses`.

```json
{ "identity": "bookings@brightwell.example", "item": "vit_01JAD4K8V4QW7X2M5N6P8R0T1Y",
  "operations": ["fill"], "purpose": "Check PCN status each morning." }
```

The answer is the grant, `status: "pending_approval"`. An identical pending or active grant gives
`vault_grant_exists` with its `grant_id`. There is no tool to approve.

### `vault_list_grants`

Arguments: `identity`, `status`, `item`, `limit`, `cursor`.

### `vault_get_use`

Arguments: `use`. Returns `status`, `operation`, `phases` with which are done, `close_reason`, and the reported
outcome (`signed_in`, `credentials_rejected`, …). Use it to tell your user what happened; the use's ticket is
never shown.

### `vault_sign`

P1. Arguments: `identity`, `item`, and either `message_base64` (HMAC-SHA256) or `method`, `url`, `headers`,
`body_sha256` and `timestamp` (AWS SigV4). Returns the signature, or `status: "pending_approval"` when the
item's approval level needs a person; then poll `vault_get_use`. A signature is not the secret, so it may reach
the model.

## 3. In "### `mail_request_account`", add after the arguments paragraph

`vault_login` (optional): `{ origins, password: { generate: {} }, email_otp, approval }`. When a person approves
the entry, the vault creates a login the agent can use to sign in at those sites, with a generated password the
agent never sees, and grants the agent `fill` there for 90 days ([Vault](../guides/vault.md)).

## 5. Paragraph for "## Choose a key for an agent"

For the vault, give the agent's identity key `vault:read` and `vault:request` (and nothing else from the vault
family: identity keys cannot hold `vault:use`, `vault:approve` or `vault:admin`). Your server starts fills with
its own key, and your consumers receive values with their own keys.
