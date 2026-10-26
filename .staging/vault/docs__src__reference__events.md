Where this goes: `docs/src/reference/events.md`. A new subsection of "## Event types" after "### Service accounts" (after the phone subsection, if pasted first). Source: [Vault § 14.4](../project/design/vault.md#144-events).

### Vault

Platform events with `tenant_id` set, and `identity_id` set where the change concerns one identity. Every
`data` carries `audit: { "seq", "row_hash" }`, the head of the tenant's hash-chained vault audit after the
change, so an integrator that stores its events holds an independent copy of the chain heads. No event carries
a value, a ticket, a packet, an unlock key or a placeholder. `item` is the [vault item](api.md#vault) as the
API returns it (with `title` and `username`, which are not secrets; filter by type if you want neither in a
webhook), `grant` and `use` likewise (a `use` never carries its ticket). `vault.item.*`, `vault.grant.*`,
`vault.use.*` and `vault.ticket.*` need `vault:read`, and identity keys see those of their own identity;
`vault.consumer.changed` and `vault.key.changed` need `vault:admin` and are never shown to identity keys.

| Type | When | `data` (besides `audit`) |
|---|---|---|
| `vault.item.created` | An item was created or imported | `item` (with `import.source` for an import) |
| `vault.item.updated` | Its metadata, origins, approval level or state changed | `item` |
| `vault.item.rotated` | A new version became current (rotation, write-back, `rotate_fill`) | `item` |
| `vault.item.deleted` | It was crypto-shredded | `item_id` and the shred receipt |
| `vault.item.revealed` | A person revealed a value in the console, or was shown a new managed value once | `item_id`, `actor_user_id`, `rotated` |
| `vault.item.health_changed` | Its health changed | `item_id`, `health`, `health_reason` |
| `vault.grant.requested` | An agent asked for a grant | `grant` |
| `vault.grant.approved` | A person (or a key under `vault.key_approval`) approved it, or created it approved | `grant` |
| `vault.grant.denied` | It was denied | `grant` |
| `vault.grant.revoked` | It was revoked, or given up by its agent | `grant` |
| `vault.grant.expired` | Its pending week or active period ended | `grant` |
| `vault.use.requested` | The orchestrator created a use (also for refusals at creation, with `outcome: "refused"`) | `use` |
| `vault.use.approval_requested` | A person must decide | `use` |
| `vault.use.approved` | It was approved; mint a ticket now | `use` |
| `vault.use.denied` | It was denied | `use` |
| `vault.use.expired` | Its approval deadline or first-redemption window passed | `use` |
| `vault.use.canceled` | The orchestrator canceled it | `use` |
| `vault.use.closed` | It closed, normally or early | `use` (with `close_reason`) |
| `vault.ticket.issued` | A ticket was minted | `use_id`, `item_id`, `consumer_id` |
| `vault.ticket.redeemed` | A consumer redeemed a phase | `use_id`, `item_id`, `consumer_id`, `phase` |
| `vault.ticket.refused` | A redemption was refused | `use_id`, `item_id`, `consumer_id`, `phase`, `reason` |
| `vault.ticket.reported` | A consumer reported an outcome | `use_id`, `item_id`, `consumer_id`, `phase`, `outcome` |
| `vault.consumer.changed` | A consumer was registered, activated, suspended, resumed, re-keyed or revoked | `consumer_id`, `change` |
| `vault.key.changed` | The tenant key rotated, custody changed, or `revoke-all` ran | `change` (`rotated`, `custody_changed`, `revoke_all`), `custody` |

Twenty-four types. Subscribing an endpoint to `vault.consumer.changed` or `vault.key.changed` needs `vault:admin`,
and to the others `vault:read`.
