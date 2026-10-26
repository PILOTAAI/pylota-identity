Where this goes: `docs/src/project/design/security.md`. (1) Rows in "### 6.1 Inventory"; (2) a row in "### 4.7 Unauthenticated routes"; (3) rows in the router table of "### 5.1"; (4) a paragraph at the end of "### 3.3 TB3: agent → MCP"; (5) entries for `crates/core/src/sealed.rs` in "### 6.2". Source: [Phone numbers § 23](phone-numbers.md#23-security-t2).

## 1. Secret inventory rows

| `PM_TELNYX_API_KEY` | Every Telnyx call | The operator in the Telnyx portal; uploaded by `idvy setup phone` | Buys numbers and sends on the operator's prepaid account until rotated in the portal and here |
| Per-tenant SIP credentials (`phone_tenants.sip_sealed`) | The tenant's credential connection, given to Vapi for the `vapi` route | The Worker, `platform::Rng`, sealed under `PM_MASTER_KEY` | Calls on one tenant's connection, bounded by its outbound voice profile |
| Vapi API keys (`phone_voice_connectors.api_key_sealed`) and server secrets (`phone_voice_routes.server_secret_sealed`) | The tenant's Vapi organisation | The tenant (key); the Worker (secret) | The tenant's Vapi account |
| KYC data keys (`phone_compliance_documents.key_sealed`) | One per document | The Worker | Decrypts one document (needs R2 too) |
| Sealed profile fields (`phone_compliance_profiles.tax_ids_sealed`, `representative_sealed`) and port-in details (`phone_requests.secrets_sealed`) | Submission to the provider only | The tenant | Tax IDs, the representative's contact and birth data, a losing carrier's account PIN |
| Port-out PINs | Vault managed secrets ([Vault § 13.2](vault.md#132-managed-secrets)), never in D1 | The Worker | A port-out without the tenant noticing (the port-out guard still alerts and a person decides) |

`PM_TELNYX_PUBLIC_KEY` verifies webhooks and is not secret.

## 2. Unauthenticated route

| `POST /hooks/telnyx` | Ed25519 signature over `telnyx-timestamp|raw body`, a 5-minute window, in every environment; 256 KB; its own rate-limit bucket by client network |

## 3. Router table rows

Every phone route of [Phone numbers § 28](phone-numbers.md#28-mcp-and-cli) with its permission and key levels;
the decisions reserved for people (`Decision::ApprovePhoneRequest`, `Decision::DecidePortOut`,
`Decision::SetPhoneBudget`) are checked by `key_may_decide` after the permission. Identity keys can never hold
`phone:approve`, `phone:manage` or `phone:compliance`.

## 4. In "### 3.3 TB3: agent → MCP"

**Phone.** An agent can buy a number only inside a standing budget a person set, never through a flag it
passes itself (`phone_buy_number` has no confirmation argument); it can never approve, port, release outside a
scheduled expiry, submit KYC, change a voice route, set a budget or see a PIN. SMS text and caller speech are
untrusted input, screened like mail; codes for vault logins never reach a model. A drafted registration text is
untrusted, shown escaped to the person who submits it.

## 5. Sealed-column registry

Add `phone_tenants.sip_sealed`, `phone_compliance_profiles.tax_ids_sealed`,
`phone_compliance_profiles.representative_sealed`, `phone_compliance_documents.key_sealed`,
`phone_requests.secrets_sealed`, `phone_voice_connectors.api_key_sealed` and
`phone_voice_routes.server_secret_sealed`, so `idvy secrets rotate-master` re-seals them.
