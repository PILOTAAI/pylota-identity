Where this goes: `docs/src/project/design/event-feed.md`, "## 4. Who sees which events", rows in the family-permission table. Source: [Vault § 14.4](vault.md#144-events).

| `vault.item.*`, `vault.grant.*`, `vault.use.*`, `vault.ticket.*` | `vault:read` | own identity |
| `vault.consumer.changed`, `vault.key.changed` | `vault:admin` | never |

A durable consumer or endpoint subscribing to these types needs the same permissions to be created (the J29
rule).
