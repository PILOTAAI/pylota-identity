# Webhook events

Pylota Identity delivers events to your HTTPS endpoints using
[Standard Webhooks](https://www.standardwebhooks.com/). Configure endpoints through the
[API](api.md#webhooks) or `idvy webhooks create`.

## Delivery

```http
POST /webhooks/mail HTTP/1.1
Content-Type: application/json
User-Agent: PylotaIdentity/1.0 (+https://github.com/PILOTAAI/pylota-identity)
webhook-id: evt_01J9Z5…
webhook-timestamp: 1791540000
webhook-signature: v1,K5oZfzN95Z9UVu1EsfQmfVNQhnkZ2pj9o9NDN/H/pI4= v1,Hq3…(second during rotation)
```

### Verify every request

1. Build the signed content: `{webhook-id}.{webhook-timestamp}.{raw body}`. Use the raw request body
   bytes, not re-serialised JSON.
2. Compute `base64(HMAC-SHA256(secret_bytes, content))`. `secret_bytes` is the base64-decoded part of
   `whsec_…` after the prefix.
3. Compare it, in constant time, with each `v1,` signature in `webhook-signature`. Accept on any match.
4. Reject the request if `webhook-timestamp` is more than 5 minutes from your clock.
5. Deduplicate on `webhook-id`: delivery is **at least once**.

Standard Webhooks libraries exist for most languages. `idvy webhooks verify` checks a captured
request locally.

### Responses and retries

- Any `2xx` within 15 seconds counts as delivered. Response bodies are ignored and read only up to 4 KB.
- Anything else, including a timeout, TLS error, DNS error, redirect or `3xx`, is a failure. Failures
  retry at about **30 s, 2 min, 10 min, 30 min, 1 h, 2 h, 4 h, 8 h, 12 h, 12 h, 12 h, 19 h**, which
  adds up to about 72 hours. Each delay has ±10% jitter.
- After the last attempt, the delivery is marked `dead`. The event can be replayed with
  `POST /v1/webhooks/{id}/replay` for 30 days from its `occurred_at` (or `retention.events_days`, if
  shorter), counted from when the event happened, not from when the delivery went `dead`.
- After 100 consecutive failures spread over at least 24 hours, the endpoint is disabled
  (`disabled_reason: failing`) and a `webhook.disabled` event goes to the platform's endpoints and, for
  an endpoint of a partner (a partner endpoint, or an endpoint of one of the partner's tenants), to that
  partner's other endpoints; never to tenant endpoints. `webhook.test` is delivered once and is never
  replayed.
- `410 Gone` from an endpoint disables it immediately.

### Ordering

Events are not guaranteed to arrive in order. Each payload carries `occurred_at`, and a `sequence` that
increases strictly per owner: per identity for mailbox events (`message.*`, `identity.*`,
`verification.received`, `calendar.received`, `draft.*`, `suppression.created`, `quota.warning`, `sms.*`, `call.*`), per domain for `domain.*` events, and
per job for `erasure.*` and `export.*` events, and for `identity.deleted`, which the erasure job emits
after the mailbox is gone (it still carries `identity_id`, so identity-filtered endpoints receive it). Use it to discard stale updates, for example a
`message.deferred` arriving after `message.delivered`. Platform events (`webhook.disabled`,
`webhook.test`, `member.*`, `billing.*`, `tenant.policy_updated`, `account.*`, `signup.verified`, `phone_request.*`, `phone_number.*`,
`phone_registration.*`, `phone_compliance.*`, `phone_consent.changed`, `phone_spend.warning`,
`phone_setup.status_changed`, `phone_budget.changed` and `vault.*`) have
`sequence: null`.

`message.screened` always precedes, by one `sequence`, the `message.received` or `message.quarantined` it
causes, and `message.released` precedes its `message.received`. `message.received` is emitted exactly once
per message (re-parse re-emissions excepted).

The same events can also be read in order from the tenant's event log
([REST API › Event feed](api.md#event-feed)): positions follow the order of appends, not `occurred_at`, and
an event is in the log before its first webhook delivery is queued.

## Envelope

```json
{
  "id": "evt_01J9Z5…",
  "type": "message.received",
  "api_version": "2026-10-01",
  "occurred_at": "2026-10-09T10:12:03.412Z",
  "tenant_id": "ten_01J9…",
  "identity_id": "idn_01J9…",
  "sequence": 1842,
  "data": { }
}
```

`identity_id` is `null` for events that do not belong to one identity (domain, job and platform
events); `identity.deleted` is the one job event that sets it. `tenant_id` is `null` only for deployment-level platform events.

Payloads are **thin**. They carry IDs, a summary, verdicts and up to `policy.webhook_text_bytes` of
`extracted_text` (default 16 KB, maximum 64 KB). Fetch anything else through the API.

Because the `message.*`, `verification.received` and `calendar.received` payloads carry mail, an endpoint
that receives them can only be created, re-pointed or replayed by a key that also holds `messages:read` (and
`quarantine:review` for `message.quarantined`) ([REST API › Webhooks](api.md#webhooks),
[J29](../project/edge-cases.md)). An endpoint subscribed to `message.screened` also needs
`quarantine:review`, because a verdict can reveal a held message. Events of the deployment's system identity
go to platform endpoints only.

## Event types

### Messages

| Type | When | `data` |
|---|---|---|
| `message.received` | An inbound message is stored and visible | `message` (summary, see below), `thread_id`, `trust` (with the authentication detail: `spf_domain`, `dkim_domains`, `dmarc_domain`, `arc_sealer` and the rest), `extracted_text`, `extracted_text_truncated`, `attachments[]` (id, filename, content type, size); plus `reprocessed: true` when a re-parse emits it again ([J3](../project/edge-cases.md)): it is not new mail; `screening` (the record or `null`); `released: true` when a release emitted it: it is the first time this message is visible |
| `message.quarantined` | An inbound message is stored but quarantined | as `message.received`, plus `quarantine_reason`. No `extracted_text`. `reprocessed: true` as for `message.received`; `screening`; `rescreened: true` when a late signal moved a delivered message back (a re-parse that does both emits one event with both flags, after `message.screened`) |
| `message.released` | A quarantined message was released | `message`, `released_by_key_id` (API release) or `released_by_user_id` (console release; the other is `null`), `reason`. Followed in the same transaction by `message.received` (`released: true`) when the message was never evented as received |
| `message.triaged` | Triage finished (or failed) | `message_id`, `thread_id`, `triage` (with `version`, the triage logic version, and `run`, which counts completed runs) |
| `message.screened` | A screening verdict was reached (or decided at ingest), or a late signal changed it | `message_id`, `thread_id`, `message_status`, `quarantine_reason`, `late`, `screening` (the record). No subject, address or text ([Screening hold §20](../project/design/screening.md#20-events)) |
| `message.sent` | The transport accepted an outbound message, or a person resolved an uncertain send as `sent` | `message`, `provider`, `provider_message_id` (`null` after a resolve), `sent_via_fallback` |
| `message.delivered` | A recipient's server accepted it | `message_id`, `recipient`, `smtp_code` |
| `message.deferred` | A temporary failure; the provider is retrying | `message_id`, `recipient`, `smtp_code`, `smtp_response` |
| `message.bounced` | A permanent failure, or retries exhausted | `message_id`, `recipient`, `bounce_type` (`hard` or `soft`), `smtp_code`, `smtp_response`, `suppressed` |
| `message.complained` | A recipient reported spam | `message_id`, `recipient`, `suppressed: true` |
| `message.rejected` | The transport refused it, at submission or, for some recipients, when the recipient's server rejected it after submission | `message_id`, `reason`, `detail` |
| `message.failed` | It could not be sent | `message_id`, `reason` |
| `message.uncertain` | The outcome is unknown; never resent automatically | `message_id`, `reason` (`transport_timeout`, `transport_connection_lost` or `provider_outcome_unknown`), `fix` |
| `message.reconciled` | An uncertain send was matched to a provider event | `message_id`, `status` |
| `message.suppressed` | Every recipient is suppressed | `message_id`, `recipients[]` |
| `message.canceled` | Cancelled while queued | `message_id` |
| `message.labels_changed` | Opt-in. Labels of visible messages changed through the API, MCP or the console | `messages[]` (`message_id`, `thread_id`; at most 100), `added`, `removed`, `actor` (`kind`, `key_id`, `user_id`), `source` (`message`, `thread` or `batch`), `request_id`, `part`, `parts` |
| `verification.received` | A verification code or link was found in authenticated mail | `message_id`, `sender_domain`, `kind` (`code` or `link`), `account_id` (the approved service-ledger entry that matched, or `null` when the tenant does not require approval). The value itself is only available through `wait` |

The **message summary** used in `data.message` is:

```json
{
  "id": "msg_01J9…", "thread_id": "thr_01J9…", "direction": "inbound", "status": "received",
  "from": { "address": "jo@example.net", "name": "Jo Rivera" },
  "to": [ { "address": "bookings.acme@agents.example", "name": "" } ],
  "cc": [], "delivered_to": "bookings.acme@agents.example", "is_primary_recipient": true,
  "subject": "Change of dates for BK-2291", "sent_at": "…", "received_at": "…",
  "kind": "normal", "labels": [], "in_reply_to": "…", "draft_id": null, "flags": []
}
```

`draft_id` is the draft an outbound message was sent from, or `null`.

**Opt-in events.** `message.labels_changed` is written only while the tenant's policy has
`events.labels_changed: true`, and is delivered only to endpoints that name it: `*` never includes it. It
is never emitted for state the service sets (labels given at send time, erasure, expiry, release), and never
names a message that is not visible ([Parity additions § 4](../project/design/parity-additions.md#4-label-change-events-fr-wh-6)).

### Calendar

| Type | When | `data` |
|---|---|---|
| `calendar.received` | A message with a calendar object first becomes visible as `received`: when it is stored, released from quarantine or screening, or when a re-parse changed its object. Never for quarantined, screening, hidden or throttled mail, or the system identity | `message_id`, `thread_id`, `object_status`, `method`, `uid`, `sequence`, `recurrence_id`, `verified`, `verified_by`, `reason`, `organized_here`; `released: true` or `reprocessed: true` when it comes from a release or a re-parse. The `uid` is text the sender chose; read the object with `GET …/messages/{message_id}` |

### Drafts

Mailbox events with the identity's `sequence` ([Drafts](../project/design/drafts.md#15-events)). They carry
no subject, body, address, filename or note, so an endpoint needs only `webhooks:manage` to receive them.
`draft` is the draft summary: `id`, `identity_id`, `draft_kind`, `kind`, `status`, `revision`, `digest`,
`recipient_count`, `attachment_count`, `send_at`, `in_reply_to`, `forward_of`, `thread_id`, `client_id`.

| Type | When | `data` |
|---|---|---|
| `draft.approval_requested` | A draft waits for approval, or was edited while waiting | `draft`, `reason` (`requested`, `new_recipients`, `external`, `always`), `requested_by_key_id` |
| `draft.approved` | A revision was approved | `draft`, `send`, `approved_by_key_id` or `approved_by_user_id`, `on_behalf_of` |
| `draft.rejected` | An approver rejected it, or the request expired after 7 days | `draft`, `reason` (`operator` or `expired`), `rejected_by_key_id` or `rejected_by_user_id`, `on_behalf_of` |
| `draft.sent` | The draft became a message | `draft`, `message_id`, `thread_id`, `message_status` (`queued` or `suppressed`) |
| `draft.failed` | A scheduled or approved send ended without a message | `draft`, `reason`, `attempts` |

### Identities and addresses

| Type | `data` |
|---|---|
| `identity.created` | `identity` |
| `identity.updated` | `identity`, `changed` (list of field names) |
| `identity.paused` | `identity_id`, `reason` (`manual`, `abuse_threshold` or `tenant_suspended`), `metrics` (for abuse) |
| `identity.resumed` | `identity_id` |
| `identity.deleted` | `identity_id`, `erasure_request_id`. Emitted once, when the identity-scope erasure that deletes the identity completes and its status becomes `deleted` (after any legal holds end). It comes from the erasure job, so its `sequence` is the job's, and `identity_id` is set |
| `identity.address_added` | `address` |
| `identity.address_activated` | `address` (pending → active once its domain is `healthy` or `degraded`) |
| `identity.address_promoted` | `address`, `previous_primary` (with `retire_at`) |
| `identity.address_retired` | `address` |
| `identity.key_created` | `identity_id`, `kid`. A signing key was created for an identity that had no active key, lazily by a signing request or by `POST …/keys` ([Identity keys](api.md#identity-keys-and-signatures)). A rotation's new key emits `identity.key_rotated` instead |
| `identity.key_rotated` | `identity_id`, `kid` (the new active key), `previous_kid` (the key now `retiring`, or `null` when the rotation created the first key) |
| `identity.key_revoked` | `identity_id`, `kid`. The key is `retired` and has left the identity's JWK Set. Not sent when the key was already `retired` |

### Domains

| Type | `data` |
|---|---|
| `domain.created` | `domain` |
| `domain.verified` | `domain` (verification passed: `verifying` → `healthy`, also after a re-proved suspension. A domain that reaches `degraded` first gets `domain.degraded`, then `domain.recovered` when it becomes `healthy`) |
| `domain.degraded` | `domain_id`, `issues[]` (`code`, `record`, `fix`) |
| `domain.failing` | `domain_id`, `issues[]`, `fallback_active` |
| `domain.suspended` | `domain_id`, `reason` (`failing_14_days`, `nameservers_changed`, `ownership_record_missing` or `registration_changed`) |
| `domain.recovered` | `domain_id`, `from_state` |
| `domain.reminder` | `domain_id`, `state`, `hours_in_state` (sent at 24 h, 72 h and 7 days; a domain never verified also gets one at 12 days, 288 h, two days before its unverified expiry; a `nameservers` domain still `pending` also gets a final one at 21 days, 504 h, before Cloudflare deletes the zone at 28 days) |
| `domain.removed` | `domain_id`, `reason`: `requested` (removed through `DELETE /v1/domains/{id}`), `zone_expired` (a `nameservers` zone was never activated and Cloudflare deleted it; the domain can be added again), `evicted` (the domain was never verified, and another tenant proved control of its DNS with a claim record) or `unverified_expired` (the domain was never verified within 14 days of being added, or of its zone's activation) |

### Privacy, platform and webhooks

| Type | `data` |
|---|---|
| `erasure.completed` | `erasure_request` (with receipt) |
| `erasure.failed` | `erasure_request_id`, `step`, `error`. Sent only after the job runner retried the step until 20 hours after the first request; submit the erasure again to restart it (a tenant erasure resumes at the failed step) |
| `export.completed` | `export_id`, `expires_at` (fetch the download link from the API) |
| `suppression.created` | `address_hint`, `reason`, `source_message_id` |
| `quota.warning` | `metric` (`sends` in v1), `used`, `limit`, `scope` (tenant or identity). Sent at 80% and at 100% of a daily send cap |
| `webhook.disabled` | `webhook_id`, `reason` (platform endpoints, and the other endpoints of the disabled endpoint's partner: its own, or its tenant's) |
| `webhook.test` | `message: "hello"` |

### Workspaces, members and billing

| Type | `data` |
|---|---|
| `member.invited` | `invitation_id`, `email_hint` (masked), `role` |
| `member.joined` | `user_id`, `role` |
| `member.role_changed` | `user_id`, `from`, `to` |
| `member.removed` | `user_id` |
| `billing.plan_changed` | `from_plan`, `to_plan`, `reason` (`checkout`, `portal` (an upgrade confirmed in the Stripe Customer Portal), `scheduled_downgrade` (a downgrade the owner scheduled took effect at the period end), `payment_failed_grace_ended`, `payment_recovered` (the plan was restored after a late payment, or when a dispute closed in the workspace's favour), `canceled`, `operator`, `dispute` (a payment was disputed: the default plan applies and sends stop until the dispute closes)) |
| `billing.payment_failed` | `grace_until` |
| `billing.limit_reached` | `feature`, `granted`, `resets_at` (sent once per feature per period, when the first `402` is returned) |
| `signup.verified` | `signup_id`, `address`, `verified_via` (`code`, `link` or `claim`), `user_id`. A provisional agent sign-up was verified; a platform event with `tenant_id` and `identity_id` set ([Agent self-sign-up](../project/design/agent-signup.md#18-events-and-audit)) |
| `tenant.policy_updated` | `fields` (the dotted paths written), `by` (`platform`, `partner`, `tenant` or `console`), `actor_key_id`, `actor_user_id` (one of them `null`), `policy_version`. Sent for every policy write except a tenant's creation, to the tenant's, its partner's and platform endpoints ([Workspace policy](../project/design/workspace-policy.md#7-audit-and-events)) |

### Service accounts

Platform events with `tenant_id` and `identity_id` set, so an endpoint's identity filter applies
([Service sign-up ledger](../project/design/service-accounts.md#6-events)). `account` is the
[service account](api.md#service-accounts) as the API returns it.

| Type | When | `data` |
|---|---|---|
| `account.requested` | An agent recorded that it wants an account at a service | `account` |
| `account.approved` | An operator approved the entry | `account` |
| `account.rejected` | An operator rejected it, or it stayed undecided for 7 days | `account`, `reason` (`operator` or `expired`) |
| `account.closed` | The entry was closed, or deleted while it was pending or approved | `account` |

### Phone numbers

**Mailbox events** carry the identity's `sequence`, like `message.*`. SMS text and transcripts are untrusted
content; verification codes are never in an event (an SMS text shows them as `[code]`), and an SMS withheld for
the vault (`vault_code`) emits nothing. Counterparty numbers appear in `sms` and `call` only where a mail event
would carry an address; masked hints elsewhere.

| Type | When | `data` |
|---|---|---|
| `sms.received` | An inbound SMS is stored and visible | `sms` (summary below), `conversation_id`, `trust` (`sender_unverified: true`, `screening`), `media[]` (`n`, content type, size) |
| `sms.quarantined` | An inbound SMS is stored but held | As `sms.received` without `text`, plus `quarantine_reason` (`account_unapproved`, `otp_unsolicited`, `steering_suspected`, `screening_unavailable`, `risky_attachment`) |
| `sms.released` | A held SMS was released | `sms_id`, `released_by_key_id` or `released_by_user_id` |
| `sms.screened` | A screening verdict on an SMS | `sms_id`, `status`, `quarantine_reason`, `screening` |
| `sms.verification_received` | A verification code was found in a visible SMS | `sms_id`, `number_id`, `sender_hint`, `kind` (`code`), `account_id`. The value is only available through `GET …/sms/wait` |
| `sms.sent` | The provider accepted the SMS | `sms_id`, `segments`, `sender_used`, `sender_substituted` |
| `sms.delivered` | The handset or carrier confirmed delivery | `sms_id` |
| `sms.failed` | The SMS was refused or not delivered | `sms_id`, `reason` |
| `sms.delivery_unconfirmed` | The carrier gave no final answer | `sms_id` |
| `sms.uncertain` | The outcome is unknown; never resent automatically | `sms_id`, `reason`, `fix` |
| `sms.reconciled` | An uncertain SMS was matched to a provider record | `sms_id`, `status` |
| `call.started` | A call on one of the identity's numbers began | `call` |
| `call.ended` | It ended, or a call record filled it in later | `call` (with `duration_seconds`, `outcome`, `cost_pence`, `source`: `webhook` or `cdr`) |

The **SMS summary** in `data.sms`:

```json
{ "id": "sms_01JB…", "conversation_id": "smc_01JB…", "number_id": "pnm_01JB…", "direction": "inbound",
  "status": "received", "from": "+447700900456", "to": "+447700900123", "text": "Can I collect at 3?",
  "segments": 1, "client_reference": null, "received_at": "…", "sent_at": null, "flags": ["sender_unverified"] }
```

The **call** object: `id`, `number_id`, `direction` (`inbound`, `outbound`), `counterparty`, `route_kind`,
`started_at`, `answered_at`, `ended_at`, `duration_seconds`, `outcome` (`answered`, `no_answer`, `busy`, `failed`,
`rejected`), `disclosure_applied`, `cost_pence`, `source`.

**Platform events** have `sequence: null`, with `tenant_id` and `identity_id` set where the subject belongs to an
identity. `phone_request` is the [phone request](api.md#phone-requests) as the API returns it to the endpoint's
level (provider costs only to platform and partner endpoints).

| Type | When | `data` |
|---|---|---|
| `phone_request.requested` | A request was created, or returned to `pending_approval` because its price rose | `phone_request`, `reason` (`created` or `quote_changed`) |
| `phone_request.approved` | A person, an allowed key or the tenant's standing budget approved it | `phone_request`, `approved_by` (`person`, `key`, `standing_budget`, `purchase`), `approved_by_key_id` or `approved_by_user_id`, `on_behalf_of`, `budget_version` (with `standing_budget`) |
| `phone_request.rejected` | Rejected, or expired after 7 days | `phone_request`, `reason` (`operator` or `expired`) |
| `phone_request.canceled` | Canceled by the requester, or by erasure | `phone_request`, `reason` |
| `phone_request.completed` | The provider work ended successfully | `phone_request`, `result` |
| `phone_request.failed` | The provider work ended without success | `phone_request`, `reason` |
| `phone_request.uncertain` | A billable call's outcome is unknown; the service is reconciling | `phone_request` |
| `phone_number.status_changed` | A number changed status | `number_id`, `identity_id`, `from`, `to`, `setup_state`, `provisioning` (`status`, `code`, `retryable`), `reason` (for example `moved`, `adopted`, `released`, `missing_at_provider`) |
| `phone_number.capability_changed` | One of the number's capability facets changed state (for example `outbound_sms` from `activating` to `enabled` when a US registration became ready) | `number_id`, `identity_id`, `capability` (`outbound_sms`, `inbound_sms`, `inbound_calls`, `outbound_calls`), `from`, `to`, `reason`, `next_step` |
| `phone_number.expiring` | A number bought with `expires_at` will be released within 24 hours | `number_id`, `identity_id`, `release_at` |
| `phone_number.action_needed` | The provider needs information before a deadline | `number_id`, `action_needed` (`kind`, `message` as untrusted text, `deadline_at`) |
| `phone_number.port_out_requested` | Another carrier asked for the number | `number_id`, `port_out_id`, `respond_by` |
| `phone_number.port_out_decided` | A person authorised or rejected it, or the provider authorised it | `number_id`, `port_out_id`, `decision` (`authorized`, `rejected`, `provider_authorized`), `decided_by_key_id` or `decided_by_user_id` |
| `phone_registration.status_changed` | A US 10DLC brand, campaign or number assignment, or a toll-free verification, changed status | `registration_id`, `kind`, `from`, `to`, `failure_reason` |
| `phone_compliance.status_changed` | The compliance profile's status for a country, number type and action changed | `country`, `number_type`, `action`, `from`, `to` (no personal data) |
| `phone_consent.changed` | A counterparty opted out or in | `number_hint` (masked), `state`, `purpose`, `source` |
| `phone_spend.warning` | Phone spend reached 80% or 100% of a monthly cap | `scope` (`tenant`, `identity`, `registration`), `identity_id`, `percent`, `used_pence`, `cap_pence`, `resets_at` |
| `phone_setup.status_changed` | The tenant's onboarding status for a country changed | `country`, `from`, `to`, `next_step` (no personal data) |
| `phone_budget.changed` | The tenant's standing purchase budget was set, loosened, tightened or turned off | `version`, `enabled`, `change` (`set`, `loosened`, `tightened`, `disabled`), `decided_by` (`person`, `key`) |

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

## Versioning

`api_version` is the payload schema date. Additive changes, such as new fields or new event types, keep
the version. v1 has one version, `2026-10-01`, so there is nothing to pin and endpoints have no
version field. A breaking change would get a new `api_version`, and the old one would stay available for
12 months; the endpoint field that selects a version is added with that change, through an ADR.

## Reading events without an endpoint

Every event that webhooks deliver for a tenant is also kept, with the same envelope and `evt_` ID, in the
tenant's event log for `retention.events_days` ([Event feed and live stream](../project/design/event-feed.md)):

- `GET /v1/events`: pages after a cursor, with a long poll of up to 60 seconds;
- durable event consumers: a named position kept for you, with commit and redelivery;
- `GET /v1/events/stream`: a WebSocket that pushes events and resumes from a cursor with nothing lost.

Endpoint filters do not apply to the log. A key sees an event only when it could read its subject through
the API (SMS and call events need `phone:read`, held SMS events also `quarantine:review`; phone
request, number, registration, consent, spend, setup and budget events need `phone:read`, and phone compliance
events `phone:compliance`; vault events need `vault:read`, and `vault.consumer.changed` and `vault.key.changed`
need `vault:admin`), and events of the system identity are never in a log. Deduplicate on `id` across every surface.
