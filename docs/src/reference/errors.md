# Errors

Every non-2xx response has this body:

```json
{
  "error": {
    "code": "rate_limited",
    "message": "Too many requests for this API key.",
    "retryable": true,
    "fix": "Wait for the number of seconds in the Retry-After header, then retry with the same Idempotency-Key.",
    "docs": "https://pylotamail.com/docs/reference/errors#code-rate_limited",
    "request_id": "req_01J9Z4…",
    "details": { "retry_after": 12 }
  }
}
```

| Field | Meaning |
|---|---|
| `code` | Stable, machine-readable, `snake_case`. New codes can be added, so handle unknown codes by HTTP status |
| `message` | Human-readable. It can change. Never parse it |
| `retryable` | `true` means the same request can succeed later unchanged. Retry with backoff, and with the **same** `Idempotency-Key` for sends. `false` means the request has to change |
| `fix` | One sentence telling a developer, or an agent, what to do |
| `docs` | A link to this code's entry on this page, as published by the deployment (`PM_DOCS_URL`) |
| `request_id` | Quote it in bug reports. It is also in the `Request-Id` header |
| `details` | Optional, code-specific |

A `404` always carries one of this service's own codes. A `404` without this envelope came from
something else (a proxy, a wrong host) and must **not** be read as "already deleted". An envelope from this
service always carries `docs`.

## How a client should retry

```text
if status == 2xx                              → done
if the body is this service's error envelope:
    error.retryable == false                  → do not retry; fix the request
    error.code == "rate_limited"              → sleep Retry-After, retry
    error.retryable == true                   → retry with backoff
if network error, timeout, or 502/503/504 without the envelope (the outcome is unknown):
    a GET, or a request with an Idempotency-Key → retry with backoff
    a POST that stores no result (a search, batch-get, a signing call, a stream-token mint)
                                              → may be repeated
    anything else (POST /v1/agent-signup, the mailbox restore, PATCH, DELETE, a PUT without a key)
                                              → do not repeat it blindly; read the resource first
any other answer without the envelope         → not from this service; do not retry
backoff: 0.5 s, 1 s, 2 s, 4 s … plus jitter, cap 60 s; stop after a bounded number of retries
for sends: ALWAYS reuse the same Idempotency-Key on every retry of the same message
```

A network error on a send is always safe to retry with the same key. You get the original result back,
whether or not the first attempt reached the server.

The TypeScript SDK, the Rust SDK's `RetryPolicy::standard()` and `pmail` follow this decision order with
at most 3 retries by default (0.5 s, 1 s, 2 s plus up to 250 ms of jitter) and `Retry-After` slept for at
most 60 s. After a network error the TypeScript SDK repeats only a `GET` or a request with an
`Idempotency-Key`; any other request, a `POST` that stores no result included, ends with
`OutcomeUnknownError`, as does any call whose outcome it could not learn. The error carries the
idempotency key to retry with when the request had one
([SDKs, skills and agent tooling § 2.6](../project/design/sdk-and-skills.md#26-retries-timeouts-and-rate-limits)).

## Catalogue

### Authentication and access

| HTTP | Code | Retryable | When |
|---|---|---|---|
| 401 | <a id="code-unauthenticated"></a>`unauthenticated` | no | No key, a malformed key, or an unknown key |
| 401 | <a id="code-key_expired"></a>`key_expired` | no | The key passed `expires_at` |
| 401 | <a id="code-key_revoked"></a>`key_revoked` | no | The key was revoked |
| 403 | <a id="code-permission_denied"></a>`permission_denied` | no | The key lacks the permission (`details.required` names it; where a route accepts either of two permissions, as the upload routes accept `messages:send` or `drafts:write`, `details.required` lists both and `details.any_of` is `true`; a `tenant`-scope erasure request needs `tenants:erase` too), or the action is turned off for API keys on this deployment: `release` with `PM_QUARANTINE_KEY_RELEASE=off`, unless the tenant's policy sets `quarantine.key_release: true` and the key is a platform key or the tenant's partner key (a tenant or identity key never qualifies), where only a signed-in person can release. The other two decisions reserved for people get it under the same rule, with `details.reason = "person_required"`: a tenant key loosening a guard field of the policy (`details.field`, [J25](../project/edge-cases.md)) and an API key approving a service account ([E11](../project/edge-cases.md)). `details.reason = "self_approval"`: a key approving the service account it requested ([E11](../project/edge-cases.md)). Approving a draft: `details.reason = "person_required"` where API keys may not take the decision (a key approves only while `PM_QUARANTINE_KEY_RELEASE` is `on` or `PM_CONSOLE` is `off`, or, where the tenant's `drafts.key_approval` is `true`, when it is a platform key or the tenant's own partner key; otherwise only a signed-in person approves), and `"self_approval"` when the key wrote the content it approves. A reply or forward draft created without `messages:read` gets it with `details.required` naming `messages:read`. On a durable event consumer, `details.reason = "consumer_owner_only"` for a key that is not its owner. A key of a provisional agent sign-up calling a route outside the provisional set gets it with `details.reason = "signup_unverified"` |
| 403 | <a id="code-scope_denied"></a>`scope_denied` | no | The route or field needs a higher key level than the caller's (for example tenant search with an identity key, or `transport` on `PATCH /v1/domains/{domain_id}` with a tenant key), or an identity key tried to write to its tenant's domains or webhooks. A partner key gets it on a platform-only route or field of one of its own tenants (`billing` in `POST /v1/tenants`, `PATCH /v1/tenants/{tenant_id}/billing`, `transport`), on a platform-only policy field or a lower-only policy field above its ceiling (`details.field`; [Configuration › Who may change a field](configuration.md#who-may-change-a-field)), when it sets `status: "active"` on a tenant a platform key suspended (`details.field = "status"`), and when it resumes an identity paused for `abuse_threshold` ([J17](../project/edge-cases.md)). Any key but a platform key gets it for an identity `send_policy.daily_cap` above the tenant's `identity_daily_send_cap` (`details.field = "send_policy.daily_cap"`), for a Cloudflare zone its tenant may not use on a domain create (`details.reason = "zone_not_allowed"`, [H8](../project/edge-cases.md)), and for an identity create in the default tenant (`details.reason = "default_tenant"`, [A18](../project/edge-cases.md)). Every key gets it for `DELETE` on the platform domain, which cannot be removed through the API ([H9](../project/edge-cases.md)). A workspace writer (a tenant key with `policy:write`) gets it for a policy field it may not write (`details.field`, `details.reason = "not_writable"`) and for a lower-only field above its workspace ceiling (`details.reason = "above_ceiling"`, with `details.ceiling` and `details.ceiling_source`) ([J23](../project/edge-cases.md), [J24](../project/edge-cases.md)); a partner key's refusals carry the same `reason` |
| 403 | <a id="code-key_scope_exceeded"></a>`key_scope_exceeded` | no | You tried to create a key wider than your own, or to read, rotate or revoke one: a key of a higher level, or one holding a permission you do not hold (platform and partner keys may still grant and manage `identities:sign`) ([J20](../project/edge-cases.md)) |
| 403 | <a id="code-tenant_suspended"></a>`tenant_suspended` | no | The tenant is suspended |
| 403 | <a id="code-partner_suspended"></a>`partner_suspended` | no | A platform operator suspended the key's partner. Every partner key of it, and every tenant and identity key of its tenants, gets this on every route, `GET /v1/me` included, until the partner is `active` again. The tenants' status does not change and their inbound mail is still stored; deliveries to the partner's and the tenants' endpoints are held ([J13](../project/edge-cases.md)) |
| 403 | <a id="code-partner_tenant_limit"></a>`partner_tenant_limit` | no | `POST /v1/tenants` with a partner key whose partner already has `max_tenants` tenants that are not erased (default 25; `details.max_tenants`). Erase a tenant, or ask the platform operator to raise the limit ([J18](../project/edge-cases.md)) |
| 403 | <a id="code-test_mode_recipient"></a>`test_mode_recipient` | no | A test tenant tried to send to an address that is neither on the simulator nor an identity of its own tenant or of another test tenant with the same partner; live tenants never receive loopback mail ([L3](../project/edge-cases.md)) |
| 403 | <a id="code-policy_denied"></a>`policy_denied` | no | The tenant's policy forbids the action. In v1.0: a signed HTTP request (`POST …/http-signatures`) while tenant policy `web_bot_auth.allowed` is `false`, the default ([O13](../project/edge-cases.md)); a platform operator turns it on in the tenant's policy. And `wait` with `kind=verification` for a sender domain without an approved service-ledger entry while the tenant's `accounts.require_approval` is `true` (`details.reason = "account_not_approved"`, `details.service_domain`, [E9](../project/edge-cases.md)); request an entry and wait for its approval. Also a send from a workspace on the Free plan or still in its send ramp while the platform operator's `free_sending` switch is off (`details.reason = "free_sending_off"`, [Observability › Automatic containment](../project/design/observability.md#56-automatic-containment)); and a send from a provisional agent sign-up to anyone but its named human (`details.reason = "signup_unverified"`), to that human after their address hard-bounced (`signup_human_unreachable`), or to anyone while the operator's `agent_signup` switch is off (`signup_paused`) |
| 403 | <a id="code-approval_required"></a>`approval_required` | no | A send, reply, reply-all or forward needs approval under the tenant's `drafts.require_approval` (or the identity's stricter `send_policy.require_approval`): `details.reason` is the level, `details.recipients` the addresses that made it needed. Nothing was stored. Create the message as a draft and send it; it waits for an approver ([Drafts](../project/design/drafts.md)) |
| 403 | <a id="code-signup_closed"></a>`signup_closed` | no | `POST /v1/agent-signup` while agent sign-up is closed on this deployment (`details.reason = "disabled"`), or paused by the operator's switch (`paused`), which also refuses resends and replacements ([Agent self-sign-up](../project/design/agent-signup.md)) |
| 403 | <a id="code-invalid_signature"></a>`invalid_signature` | no | An SNS message to `POST /hooks/ses` (SES delivery events) or `POST /hooks/ses/inbound` (SES inbound mail) failed verification: `SignatureVersion` not `2`, a bad signature, a signing certificate not on `sns.{PM_SES_REGION}.amazonaws.com`, another topic, or a stale `Timestamp`. Not returned to API callers |

### Validation

| HTTP | Code | Retryable | When |
|---|---|---|---|
| 400 | <a id="code-invalid_request"></a>`invalid_request` | no | Malformed JSON or schema violation. `details.errors[]` has `{path, message}`. On `POST /v1/keys`, `details.reason = "permission_not_allowed_for_level"` when `permissions` lists one the new key's level cannot hold, and `"permission_owner_only"` when a tenant key would get `tenants:erase` from a caller other than the workspace owner's session or its keys ([API › Permissions](api.md#permissions)). On `POST /v1/keys`, `details.reason = "unknown_mcp_tool"` or `"mcp_tool_not_permitted"`, with `details.tool`, for an entry of `mcp_tools` ([Y50](../project/edge-cases.md)). On `POST …/accounts`, `details.reason = "own_deployment"` when the service is this deployment's own domain ([E14](../project/edge-cases.md)). On the event feed, `details.reason` is `cursor_ahead` (a position above the log's head), `credential_in_url`, `two_credentials` or `subprotocol_required` (the stream handshake), or `tenant_id` is the path when a platform or partner key omitted it. On `POST …/feedback`, `details.reason = "feedback_not_applicable"` (with `details.screening_status`) when the verdict does not fit the record's status. `details.reason = "label_limit"` (with `details.message_ids`) when a `batch-update` would give a message more than 64 labels; `"system_identity"` for a platform key's list or retention change on the system identity; `"beyond_retention"` when a draft's `send_at` is after its expiry under `retention.message_hours` |
| 400 | <a id="code-idempotency_key_required"></a>`idempotency_key_required` | no | A send without `Idempotency-Key` |
| 400 | <a id="code-invalid_idempotency_key"></a>`invalid_idempotency_key` | no | Longer than 255 characters, or not printable ASCII |
| 400 | <a id="code-invalid_query"></a>`invalid_query` | no | The search query could not be parsed. `details.position` and `details.expected` say where and what |
| 400 | <a id="code-address_invalid"></a>`address_invalid` | no | Not a valid RFC 5321 address |
| 400 | <a id="code-address_unsupported"></a>`address_unsupported` | no | An SMTPUTF8 (non-ASCII) local part ([A3](../project/edge-cases.md)). A non-ASCII name that looks like a reserved name, is mixed-script or contains right-to-left characters gets `address_reserved` instead |
| 400 | <a id="code-address_reserved"></a>`address_reserved` | no | A reserved or confusable local part ([A4](../project/edge-cases.md)). On the shared platform domain this includes every RFC 2142 role name (`info`, `marketing`, `sales`, `support`, `abuse`, `postmaster` and the rest) where it would stand alone as the local part: usernames of the default tenant, whose suffix is empty (`support.acme@…` is not a role address). On a tenant's own domain only `postmaster` and `abuse` are reserved, because mail to them goes to the tenant's owner contact. Names such as `noreply`, `mailer-daemon` and `journal` are reserved everywhere |
| 400 | <a id="code-local_part_too_long"></a>`local_part_too_long` | no | The username and suffix leave no room for a thread token (more than 40 characters) |
| 400 | <a id="code-header_not_allowed"></a>`header_not_allowed` | no | A custom header name outside the allowed set: an `X-` name that does not match `^X-[A-Za-z0-9_-]+$`, a reserved `X-Pylota-*` or `X-AI-Generated` name, or any other name than `Importance`, `Priority`, `Sensitivity`, `Keywords`, `Comments` and `Organization`. Names are matched case-insensitively ([Limits](limits.md#mail)). A wrong value for an allowed header is `invalid_request`. On webhook endpoints (`headers`, `PATCH …/headers`), `details.reason = "webhook_header"` and `details.name`: a name that fails `^[A-Za-z0-9][A-Za-z0-9-]*$`, a Fetch forbidden request header, `Proxy-*`, `Sec-*`, a header the service sets, a client-address header or `X-Pylota-*` ([Parity additions § 8.2](../project/design/parity-additions.md#82-rules)) |
| 400 | <a id="code-marketing_requirements_missing"></a>`marketing_requirements_missing` | no | `kind: marketing` without `unsubscribe` or `consent` |
| 400 | <a id="code-too_many_recipients"></a>`too_many_recipients` | no | More than `policy.max_recipients` (hard maximum 49: Cloudflare allows 50 and one is kept for the hidden journal copy). With `details.reason = "calendar_single_recipient"` and `details.max = 1`, a calendar message on the `cloudflare` transport, when a raw send takes one recipient ([Calendar invitations §5.6](../project/design/calendar-invitations.md#56-transports)) |
| 400 | <a id="code-smtp_port_not_allowed"></a>`smtp_port_not_allowed` | no | `smtp.port` is not `465` or `587` (port `25` included: Workers cannot open outbound connections on it) on a domain create with `method: smtp_relay` or a `PATCH /v1/domains/{domain_id}` with `smtp` |
| 400 | <a id="code-spf_lookup_limit"></a>`spf_lookup_limit` | no | Adding a domain whose merged SPF record would need more than 10 DNS lookups (or more than 2 void lookups). `details.lookups` has the count; `fix` names the includes to flatten ([H2](../project/edge-cases.md)) |
| 400 | <a id="code-calendar_invalid"></a>`calendar_invalid` | no | A `calendar.ics` that cannot be read, or whose structure is not a valid iTIP message: `details.reason` is one of `parse_error` (with `details.line`), `version_missing`, `prodid_missing`, `method_missing`, `multiple_methods`, `method_unknown`, `component_not_supported`, `uid_missing`, `uid_mismatch`, `uid_too_long`, `dtstamp_missing`, `sequence_invalid`, `organizer_missing`, `organizer_inconsistent`, `attendee_not_allowed`, `attendee_missing`, `attendee_count`, `dtstart_missing`, `summary_missing`, `property_not_allowed` (with `details.property`), `rrule_invalid`, `unknown_tzid` (with `details.tzid`), `vevent_count` (an `ADD`, `REFRESH` or `COUNTER` with more than one `VEVENT`) ([V32](../project/edge-cases.md)) |
| 400 | <a id="code-calendar_organizer_mismatch"></a>`calendar_organizer_mismatch` | no | The object's `ORGANIZER` is not the address the message would be sent from. `details.organizer` and `details.from_address` (on a fallback-pinned thread, the platform address) ([V30](../project/edge-cases.md)) |
| 400 | <a id="code-calendar_attendee_mismatch"></a>`calendar_attendee_mismatch` | no | A `REPLY`, `REFRESH` or `COUNTER` whose replying `ATTENDEE` is not the `From` address. `details.attendee`, `details.from_address` |
| 400 | <a id="code-calendar_recipient_mismatch"></a>`calendar_recipient_mismatch` | no | An organizer method sent to someone who is not an `ATTENDEE`, or an attendee method not sent to exactly the `ORGANIZER`. `details.address` ([V36](../project/edge-cases.md)) |
| 400 | <a id="code-upload_size_mismatch"></a>`upload_size_mismatch` | no | An upload `PUT` whose `Content-Length` is missing or not the declared size (`details.reason = "content_length"`), that has a `Content-Encoding` (`"encoding"`), or whose body ended early (`"incomplete"`). `details.expected` and `details.received` |
| 400 | <a id="code-upload_checksum_mismatch"></a>`upload_checksum_mismatch` | no | The bytes of an upload `PUT` do not have the declared SHA-256. Nothing was stored. `details.expected` |
| 400 | <a id="code-query_range_too_wide"></a>`query_range_too_wide` | no | `GET /v1/metrics/events` asked for more than 168 hourly or 92 daily buckets, or for buckets older than those kept (192 hours, 92 days). `details.max_buckets`, `details.requested_buckets`, `details.earliest` |
| 413 | <a id="code-message_too_large"></a>`message_too_large` | no | The composed message exceeds the transport limit (5 MiB with Cloudflare; `details.reason = "transport_limit"`), or its attachments total more than 26,214,400 decoded bytes (`"attachments_total"`, whatever `large_attachments` says) |
| 413 | <a id="code-calendar_too_large"></a>`calendar_too_large` | no | `calendar.ics` is over 262,144 bytes. `details.limit` |
| 413 | <a id="code-payload_too_large"></a>`payload_too_large` | no | The request body exceeds 7 MiB, or the route's own smaller limit (`details.limit`; 4,096 bytes on `POST /v1/agent-signup`) |
| 422 | <a id="code-scope_too_large"></a>`scope_too_large` | no | A tenant search, a tenant screening summary, or tenant- or domain-scope event counts, over more than 100 identities without an `identity_ids` filter |
| 426 | <a id="code-upgrade_required"></a>`upgrade_required` | no | `GET /v1/events/stream` without `Upgrade: websocket`. Open it as a WebSocket |

### Resources and state

| HTTP | Code | Retryable | When |
|---|---|---|---|
| 404 | <a id="code-tenant_not_found"></a>`tenant_not_found`, <a id="code-identity_not_found"></a>`identity_not_found`, <a id="code-address_not_found"></a>`address_not_found`, <a id="code-domain_not_found"></a>`domain_not_found`, <a id="code-thread_not_found"></a>`thread_not_found`, <a id="code-message_not_found"></a>`message_not_found`, <a id="code-attachment_not_found"></a>`attachment_not_found`, <a id="code-webhook_not_found"></a>`webhook_not_found`, <a id="code-key_not_found"></a>`key_not_found`, <a id="code-erasure_not_found"></a>`erasure_not_found`, <a id="code-export_not_found"></a>`export_not_found`, <a id="code-suppression_not_found"></a>`suppression_not_found`, <a id="code-list_entry_not_found"></a>`list_entry_not_found`, <a id="code-member_not_found"></a>`member_not_found`, <a id="code-invitation_not_found"></a>`invitation_not_found`, <a id="code-job_not_found"></a>`job_not_found`, <a id="code-dlq_item_not_found"></a>`dlq_item_not_found`, <a id="code-partner_not_found"></a>`partner_not_found`, <a id="code-account_not_found"></a>`account_not_found`, <a id="code-draft_not_found"></a>`draft_not_found`, <a id="code-event_consumer_not_found"></a>`event_consumer_not_found`, <a id="code-upload_not_found"></a>`upload_not_found`, <a id="code-signup_not_found"></a>`signup_not_found` | no | The resource does not exist **or** is outside your scope (deliberately indistinguishable). A signed link that is bad, expired or signed by a retired key also returns its target's code (`attachment_not_found` or `export_not_found`). `key_not_found` also covers an identity signing key's unknown `kid`, and the Web Bot Auth key directory (`/.well-known/http-message-signatures-directory`) while `PM_WEB_BOT_AUTH=off`. `identity_not_found` also covers a `deleting` or `deleted` identity, and the JWK Set (`/.well-known/jwks/{identity_id}.json`) of a paused or suspended identity |
| 404 | <a id="code-route_not_found"></a>`route_not_found` | no | No route has this path on this host: a mistyped path, or an API path on the console host (or the reverse) when the two hosts differ. Not a resource code: it says nothing about any data |
| 405 | <a id="code-method_not_allowed"></a>`method_not_allowed` | no | The path is a route, but not for this method. The `Allow` header lists the methods it has. (`/mcp` answers its `405` with an empty body and `Allow: POST`, as the MCP transport expects) |
| 409 | <a id="code-idempotency_conflict"></a>`idempotency_conflict` | no | Same key, different request. `details.original_message_id` is included when known |
| 409 | <a id="code-request_in_progress"></a>`request_in_progress` | yes | Same key, and the first request is still running. Also `PATCH /v1/webhooks/{webhook_id}/headers` after three concurrent changes won |
| 409 | <a id="code-client_id_conflict"></a>`client_id_conflict` | no | Same `client_id`, different identity body |
| 409 | <a id="code-username_taken"></a>`username_taken` | no | The username is in use in this tenant |
| 409 | <a id="code-slug_taken"></a>`slug_taken` | no | Another tenant uses this slug |
| 409 | <a id="code-suffix_taken"></a>`suffix_taken` | no | Another tenant uses this address suffix, or a second tenant asked for the empty suffix |
| 409 | <a id="code-not_quarantined"></a>`not_quarantined` | no | `release` on a message that is not quarantined. `details.status` is its status; for a message still being screened (`screening`) also `details.deadline_at`: wait for the verdict |
| 409 | <a id="code-not_screened"></a>`not_screened` | no | Feedback on a message that has no screening record ([Screening hold](../project/design/screening.md#11-feedback)) |
| 409 | <a id="code-screening_pending"></a>`screening_pending` | yes | Feedback on a message whose screening verdict is still pending. `details.deadline_at` says when it will be decided at the latest |
| 409 | <a id="code-domain_not_suspended"></a>`domain_not_suspended` | no | `reprove` on a domain that is not suspended |
| 409 | <a id="code-existing_mx"></a>`existing_mx` | no | Adding a `cloudflare_zone` apex, or a `dns_records` domain, that already has MX records (none of them the expected ones), without `"replace_mx": true` ([H5](../project/edge-cases.md)) |
| 409 | <a id="code-domain_not_dedicated"></a>`domain_not_dedicated` | no | A `nameservers` domain whose name has A, AAAA or MX records, or whose `www` has a CNAME, A or AAAA record, without `"confirm_dedicated": true`. Moving the nameservers would stop that website or mail. `details.records` lists what was found |
| 409 | <a id="code-zone_hold"></a>`zone_hold` | no | Cloudflare refused to create the zone because of a zone hold (`nameservers`, `delegated_subdomain`). The fix asks the customer to release the hold (for subdomains) |
| 409 | <a id="code-owner_required"></a>`owner_required` | no | Tried to remove or demote the workspace owner; transfer ownership first |
| 409 | <a id="code-plan_managed_by_stripe"></a>`plan_managed_by_stripe` | no | Tried to set a plan on a workspace whose plan is paid through Stripe |
| 409 | <a id="code-partner_has_tenants"></a>`partner_has_tenants` | no | `DELETE /v1/partners/{partner_id}` while one of the partner's tenants is not erased. `details.tenants` says how many; erase them first ([J12](../project/edge-cases.md)) |
| 409 | <a id="code-tenant_erased"></a>`tenant_erased` | no | A tenant-scope erasure request for a tenant that is already `erased`, or a platform key's `PATCH /v1/tenants/{tenant_id}` with `status` on a tenant that is `erasing` or `erased`: only the erasure job changes its status. (A tenant-scope request for a tenant still `erasing` returns the existing request with `200`.) ([I8](../project/edge-cases.md)) |
| 409 | <a id="code-system_protected"></a>`system_protected` | no | The request would delete or erase the deployment's system identity, or erase or suspend the default tenant that holds it. Nothing changed. Change the system identity with `pmail setup` (`PM_SYSTEM_FROM`) ([A17](../project/edge-cases.md)) |
| 409 | <a id="code-address_taken"></a>`address_taken` | no | The address belongs to another identity, is tombstoned, or is a console user's sign-in address ([W45](../project/edge-cases.md)) |
| 409 | <a id="code-address_is_primary"></a>`address_is_primary` | no | Tried to retire or delete the primary address |
| 409 | <a id="code-address_in_use"></a>`address_in_use` | no | Tried to delete an address that received mail (retire it instead), or to retire or delete the identity's platform address, which stays active as the fallback address for the identity's whole life |
| 409 | <a id="code-domain_not_ready"></a>`domain_not_ready` | yes | A send from a domain that is `pending` or `verifying`, or from a `pending` address. A `failing` or `suspended` domain is not an error: the send is accepted and falls back to the platform address. Also an identity create whose primary address is on a domain that is not `healthy` or `degraded`, and a `promote` to such an address |
| 409 | <a id="code-domain_in_use"></a>`domain_in_use` | no | Tried to remove a domain that still has active or retiring addresses. Also returned with `details.reason = "routing_rule_limit"` when a zone subdomain already has 200 literal routing rules and another address is added, and with `details.reason = "zone_has_domains"` (and `details.domain_ids`) when a policy write drops a zone from `domains.cloudflare_zones` while the tenant still has domains in it ([H13](../project/edge-cases.md)) |
| 409 | <a id="code-domain_exists"></a>`domain_exists` | no | The name is not available. For a domain of your own tenant, `details.domain_id` names it. Otherwise the body is the same whoever holds the name and in whatever state, and also when a zone or provider object of that name exists that this deployment did not create for you; `details.claim` is a TXT record (`_pylota-mail.{name}` = `pm-claim=…`). If you control the name's DNS, publish it and repeat the request with `"claim": true`: a holder that never verified the domain is then evicted ([H10](../project/edge-cases.md), [H14](../project/edge-cases.md)) |
| 409 | <a id="code-domain_claim_pending"></a>`domain_claim_pending` | yes | A `"claim": true` request proved control of the name's DNS, and the previous holder's unverified domain is being removed. Retry after `Retry-After` |
| 409 | <a id="code-identity_paused"></a>`identity_paused` | no | The identity is paused. `details.reason` says why (`tenant_suspended` for every identity of a suspended tenant). A paused identity also cannot sign agent assertions or HTTP requests ([O1](../project/edge-cases.md)) |
| 409 | <a id="code-identity_owner_required"></a>`identity_owner_required` | no | The identity has no accountable human |
| 409 | <a id="code-thread_busy"></a>`thread_busy` | yes | Another send holds the thread lock. Retry after `details.retry_after` |
| 409 | <a id="code-not_cancelable"></a>`not_cancelable` | no | The message is past `queued` |
| 409 | <a id="code-not_uncertain"></a>`not_uncertain` | no | `resolve` was called on a message that is not `uncertain` |
| 409 | <a id="code-auto_reply_not_allowed"></a>`auto_reply_not_allowed` | no | An auto-reply to automated mail, or over the automatic-exchange limit ([D6](../project/edge-cases.md)) |
| 409 | <a id="code-account_exists"></a>`account_exists` | no | `POST …/accounts` while the identity already has a pending or approved service-ledger entry for the same service and account identifier. `details.account_id` and `details.status` name it ([E12](../project/edge-cases.md)) |
| 409 | <a id="code-account_not_pending"></a>`account_not_pending` | no | `approve` or `reject` on a service-ledger entry that is no longer `pending_approval` (already decided the other way, closed, or expired after 7 days). `details.status` is its state. Repeating the decision it already has answers `200` instead ([E12](../project/edge-cases.md)) |
| 409 | <a id="code-draft_revision_mismatch"></a>`draft_revision_mismatch` | no | `If-Match`, or an approval's `revision` or `digest`, names a revision that is no longer current. `details.current_revision`, `details.current_etag`; `details.reason = "digest"` for a digest. Read the draft again |
| 409 | <a id="code-draft_not_editable"></a>`draft_not_editable` | yes while sending | Update, delete or reject of a draft that is being sent (`retryable: true`); update of a `sent` or `canceled` draft, and delete of a `sent` one (`details.status`). A delete of a `canceled` draft answers `200` again, and a reject of a `sent` or `canceled` one answers `draft_not_pending` |
| 409 | <a id="code-draft_not_sendable"></a>`draft_not_sendable` | yes while sending | Send of a draft that is being sent (`retryable: true`), already sent (`details.message_id`), rejected or canceled; or a request for approval of a rejected revision without a change |
| 409 | <a id="code-draft_not_pending"></a>`draft_not_pending` | no | Approve or reject of a draft in a status that cannot be decided (`details.status`): approve of a `sending`, `sent`, `rejected` or `canceled` draft, reject of a `sent` or `canceled` one; a pending approval that expired after 7 days is `rejected` |
| 409 | <a id="code-source_unavailable"></a>`source_unavailable` | no | A reply or forward draft's source cannot be used at send: `details.reason` is `erased`, `not_visible` (it is quarantined or in screening and the authority recorded for a scheduled send lacks `quarantine:review`; an API caller without it gets `404 draft_not_found` instead), `raw_expired` (its stored body was truncated and its raw MIME is gone) or `attachment_missing` |
| 409 | <a id="code-loop_detected"></a>`loop_detected` | no | A send of any `kind` whose computed hop count (`X-Pylota-Mail-Hop`) would reach the limit of 10: the thread looks like a loop between automated senders. `details.hop` is the hop it would have had ([N13](../project/edge-cases.md)). Also a reply, forward or threaded send after 25 sends in the same thread, or a reply to an address that sent the identity 25 messages, in the last 24 hours: `details.reason` is `thread_rate` or `sender_rate`, and `details.retry_after` gives the seconds until the window allows another ([N35](../project/edge-cases.md)) |
| 409 | <a id="code-sending_paused"></a>`sending_paused` | no | The tenant's sending (`details.scope = "tenant"`) or the sending domain (`details.scope = "domain"`, `details.domain_id`) was paused automatically because its complaint or bounce rate reached the provider's review level over 7 days (`details.reason = "abuse_threshold"`), or, for the tenant only, because one of its domains had a burst far above it within an hour or a day (`details.reason = "abuse_burst"`, [J33](../project/edge-cases.md)). Only a platform key can resume it ([G12](../project/edge-cases.md)) |
| 409 | <a id="code-triage_not_eligible"></a>`triage_not_eligible` | no | Re-running triage on a message that is never triaged: quarantined (release it instead, which triages it), still being screened, hidden, throttled, a delivery report or read receipt, or a message of the system identity. `details.reason` is `quarantined`, `screening` or `not_eligible` |
| 409 | <a id="code-erasure_not_completed"></a>`erasure_not_completed` | no | `POST /v1/platform/erasure-requests/{erasure_id}/reapply` for a request that did not end `completed` or `completed_with_holds`. A `queued` or `running` request erases the restored data itself; resume a `failed` one with a new request for the same scope and target |
| 409 | <a id="code-not_frozen"></a>`not_frozen` | no | `POST /v1/platform/identities/{identity_id}/restore` while the deployment is not frozen for a restore. Run `pmail ops freeze` first ([Restore from PITR](../project/design/observability.md#restore-from-pitr)) |
| 409 | <a id="code-event_consumer_name_taken"></a>`event_consumer_name_taken` | no | `POST /v1/event-consumers` with a name the tenant already uses for another key's consumer, or with other `types` or `identity_ids`. Choose another name, or repeat the original request |
| 409 | <a id="code-calendar_sequence_stale"></a>`calendar_sequence_stale` | no | An organizer method whose `SEQUENCE` is lower than one this identity already sent for the same `UID`. `details.sequence`, `details.latest_sequence`, `details.latest_message_id` ([V37](../project/edge-cases.md)) |
| 409 | <a id="code-calendar_not_invitation"></a>`calendar_not_invitation` | no | `calendar-reply` on a message that has no readable invitation (`method: "request"`), or that is outbound. `details.method`, `details.object_status` |
| 409 | <a id="code-calendar_not_verified"></a>`calendar_not_verified` | no | `calendar-reply` on an invitation that is not verified (`details.reason` is its reason), that is quarantined (`details.reason = "message_quarantined"`; release it first) or that is in screening (`"message_screening"`; wait for its verdict) |
| 409 | <a id="code-calendar_not_attendee"></a>`calendar_not_attendee` | no | `calendar-reply` on an invitation that does not name this identity as an attendee |
| 409 | <a id="code-calendar_cancelled"></a>`calendar_cancelled` | no | `calendar-reply` on an invitation that is cancelled, or that a later verified `CANCEL` covers. `details.cancel_message_id` when known |
| 409 | <a id="code-calendar_superseded"></a>`calendar_superseded` | no | `calendar-reply` on an invitation that a later verified `REQUEST` replaced. `details.latest_message_id`, `details.latest_sequence`; answer that one |
| 409 | <a id="code-calendar_event_past"></a>`calendar_event_past` | no | `calendar-reply` for an event or instance that has ended. `details.ended_at` |
| 409 | <a id="code-calendar_attendee_address_unavailable"></a>`calendar_attendee_address_unavailable` | no | `calendar-reply` when the attendee address cannot be the `From`: `details.address`, `details.reason` (`retired`, `pending`, `fallback_pinned`) |
| 409 | <a id="code-upload_not_ready"></a>`upload_not_ready` | yes | A send attaches an upload that is still `pending` or `uploading`. `details.index` and `details.status`. Finish the `PUT` first |
| 409 | <a id="code-upload_in_progress"></a>`upload_in_progress` | yes | Another `PUT` of the upload is running. Retry after `Retry-After` |
| 409 | <a id="code-upload_in_use"></a>`upload_in_use` | yes | `DELETE` of an upload a send is attaching at that moment. Retry after `Retry-After` |
| 409 | <a id="code-signup_not_pending"></a>`signup_not_pending` | no | An agent sign-up route on a sign-up that is not provisional, or past its 7 days (`details.state`). A verification of an already verified sign-up answers `200` instead |
| 409 | <a id="code-signup_cannot_verify"></a>`signup_cannot_verify` | no | The code or link was right, but the sign-up cannot be verified: the person already owns a Free workspace (`workspace_limit`; the code stays valid), the human address has become an address this deployment receives (`human_email_hosted`), or the person's account is disabled (`person_disabled`) |
| 409 | <a id="code-signup_human_unreachable"></a>`signup_human_unreachable` | no | A resend while the human's address hard-bounced. Replace the human with `PUT /v1/agent-signup/human` |
| 410 | <a id="code-raw_expired"></a>`raw_expired` | no | Raw MIME is past retention |
| 410 | <a id="code-cursor_expired"></a>`cursor_expired` | no | A pagination cursor older than 24 hours; or an event cursor, or a durable consumer's position, behind the retained event log (`details.reason`: `retention`, `size_limit` or `log_reset`; `details.oldest_cursor`; `details.consumer_id` for a consumer) |
| 410 | <a id="code-upload_expired"></a>`upload_expired` | no | The upload expired (`details.reason`: `expired`, `too_many_attempts` or `object_missing`). Upload the file again |
| 410 | <a id="code-message_expired"></a>`message_expired` | no | A retried send (same `Idempotency-Key` and body) whose message expired under the identity's `retention.message_hours`. Nothing was sent. `details.message_id`, `details.expired_at` ([Parity additions § 7.6](../project/design/parity-additions.md#76-a-retried-send-never-sends-twice)) |
| 423 | <a id="code-legal_hold"></a>`legal_hold` | no | `DELETE …/messages/{message_id}` on a message whose thread is under a legal hold. Nothing was created. Also a `PATCH` of the content, `labels` or `metadata` of a draft under a legal hold; nothing changed (a `DELETE` of such a draft still cancels it, keeping its content). Erasure requests (`POST /v1/erasure-requests`) never return it: they skip held threads and list them in the receipt |

### Policy and limits

| HTTP | Code | Retryable | When |
|---|---|---|---|
| 422 | <a id="code-all_recipients_suppressed"></a>`all_recipients_suppressed` | no | Never returned at send time: a fully suppressed send is accepted and ends `suppressed`. Returned only by a dry-run (`?dry_run=true`) |
| 422 | <a id="code-recipient_blocked"></a>`recipient_blocked` | no | A dry-run found a recipient on the send-block list |
| 422 | <a id="code-transport_unavailable"></a>`transport_unavailable` | no | The deployment or the domain's method cannot do what was asked. `details.reason` is one of `ses_not_configured` (the SES transport, `PM_SES_*`, is not configured: `dns_records`, `send_only`, `PATCH` to `ses`), `ses_receiving_not_configured` (`dns_records`, or `smtp_relay` with `inbound: ses`, without `PM_SES_INBOUND_TOPIC_ARN`, bucket and queue), `ses_identity_limit` (the SES region already has 10,000 identities), `marketing_needs_ses` (`kind: marketing` from a domain whose transport is `cloudflare`, the platform domain included: Cloudflare Email Service is for transactional mail only), `subdomain_setup_disabled` (`delegated_subdomain` while `PM_CF_SUBDOMAIN_SETUP` is not `on`), `zone_creation_not_allowed` (`nameservers` by a tenant or partner key whose tenant's policy lacks `domains.allow_create_zone: true`, or a zone create that Cloudflare refused for the Worker token's scope, as on Pylota Mail Cloud), `ses_identity_not_verified` (`PATCH` to `ses` while SES does not report the domain's identity as verified for sending with DKIM `SUCCESS`; `details.dkim_status`) or `method_not_supported` (the domain's method does not support the operation: `PATCH` with a transport it cannot use, `probe` without the `smtp` transport, `test-forwarding` without `inbound: forward`) or `calendar_needs_raw_mime` (a calendar message on the `cloudflare` transport before the raw send is available) |
| 422 | <a id="code-smtp_tls_required"></a>`smtp_tls_required` | no | The SMTP relay does not offer STARTTLS on port 587 (or TLS on 465). The credentials were not sent. Returned by domain create (`smtp_relay`) and by `PATCH /v1/domains/{domain_id}` with `smtp`; also a domain health issue |
| 422 | <a id="code-smtp_auth_failed"></a>`smtp_auth_failed` | no | The SMTP relay answered `535` to AUTH. Returned by domain create (`smtp_relay`) and by `PATCH /v1/domains/{domain_id}` with `smtp`; also a domain health issue |
| 422 | <a id="code-cf_token_required"></a>`cf_token_required` | no | `PM_CF_API_TOKEN` is not set, and a domain create with `method` `cloudflare_zone`, `nameservers` or `delegated_subdomain`, or an identity or address that needs a literal routing rule, needs it. For an apex `cloudflare_zone`, `pmail domains add --local-token` with your own Cloudflare token works instead (catch-all, no literal rules) |
| 422 | <a id="code-agentic_disabled"></a>`agentic_disabled` | no | Agentic search is turned off by tenant policy |
| 422 | <a id="code-address_limit_reached"></a>`address_limit_reached` | no | The identity already has 20 addresses in any state |
| 422 | <a id="code-unverified_domain_limit"></a>`unverified_domain_limit` | no | The tenant already holds 5 domains that were never verified (`details.limit`). Verify or remove one, or wait for one to expire 14 days after it was added ([H14](../project/edge-cases.md)) |
| 422 | <a id="code-zone_domain_limit"></a>`zone_domain_limit` | no | The Cloudflare zone already holds 30 mail domains, routing and sending together (`details.limit`); nothing was created ([H15](../project/edge-cases.md)) |
| 422 | <a id="code-account_limit_reached"></a>`account_limit_reached` | no | The identity already has 10 pending service-ledger entries, or 200 in any state (`details.limit`, `details.status`). Wait for decisions, or close or delete old entries ([E12](../project/edge-cases.md)) |
| 422 | <a id="code-draft_limit_reached"></a>`draft_limit_reached` | no | The identity already has 1,000 drafts that are not `sent` or `canceled` (`details.limit`). Send, cancel or wait for old drafts to expire |
| 422 | <a id="code-feedback_limit_reached"></a>`feedback_limit_reached` | no | The message already has 20 screening feedback rows (`details.limit`) |
| 422 | <a id="code-event_consumer_limit_reached"></a>`event_consumer_limit_reached` | no | The tenant already has 20 durable event consumers (`details.limit`). Delete one you no longer use |
| 422 | <a id="code-upload_limit_reached"></a>`upload_limit_reached` | no | The identity already has 100 live uploads, or 524,288,000 declared bytes of them. `details.limit` (`count` or `bytes`), `details.used`, `details.max`. Delete unused uploads or wait for them to expire |
| 422 | <a id="code-attachment_risky"></a>`attachment_risky` | no | An attachment (an upload, an inline attachment of a send, or an attachment of a draft when its revision is saved) has a quarantining risk (`details.risk`: `executable`, `macro`, `archive_bomb`, `encrypted_archive`, `type_mismatch`), including after a rename, or the scanner found malware (`details.reason = "infected"`). `details.index` names the attachment of a send |
| 422 | <a id="code-signup_code_invalid"></a>`signup_code_invalid` | no | The sign-up code was not accepted: `details.reason` is `wrong` (with `attempts_left` and `failures_left`), `burned` (10 attempts), `locked` (30 failures; the link still works), `expired` (24 hours, or its link key was revoked), `superseded` (a newer code exists) or `none` |
| 422 | <a id="code-signup_replace_limit"></a>`signup_replace_limit` | no | The human of an agent sign-up was already replaced twice |
| 422 | <a id="code-webhook_limit_reached"></a>`webhook_limit_reached` | no | The tenant, the partner or the platform already has 20 webhook endpoints (each has its own limit of 20) |
| 422 | <a id="code-list_limit_reached"></a>`list_limit_reached` | no | An allow or block list scope already holds 10,000 entries, or the tenant 50,000 (`details.limit`, `details.scope`: `identity`, `tenant` or `total`) |
| 422 | <a id="code-webhook_header_limit_reached"></a>`webhook_header_limit_reached` | no | A webhook endpoint would have more than 10 custom headers or more than 4,096 bytes of header values (`details.limit`, `details.reason`: `count` or `bytes`) |
| 422 | <a id="code-key_limit_reached"></a>`key_limit_reached` | no | `POST /v1/keys` for a tenant that already has 100 active (neither revoked nor expired) tenant and identity keys, or for a partner that already has 10 partner keys (`details.limit`). Revoke keys you no longer use ([J22](../project/edge-cases.md)) |
| 422 | <a id="code-web_bot_auth_disabled"></a>`web_bot_auth_disabled` | no | Signed HTTP requests are turned off on this deployment (`PM_WEB_BOT_AUTH=off`, the default): `POST …/http-signatures`, and rotating the `web_bot_auth` key with `POST /v1/platform/keys/web_bot_auth/rotate` ([O9](../project/edge-cases.md)) |
| 402 | <a id="code-billing_limit"></a>`billing_limit` | no | A plan allowance is spent (`details`: `feature`, `granted`, `used`, `resets_at`, `upgrade_url`). Nothing was stored: upgrade or add a top-up, then retry with the **same** `Idempotency-Key` |
| 429 | <a id="code-rate_limited"></a>`rate_limited` | yes | A per-key or per-identity rate limit (including signing: 600 assertions and HTTP signatures a minute per identity), the limit for all keys of one tenant or of one partner together (`details.bucket` is `tenant` or `partner`), or the per-domain limit of `verify` and `probe` (one a minute). On the event feed, `details.bucket` is `events_wait` (concurrent long polls), `stream_tokens` (live stream tokens: unredeemed and unexpired) or `streams` (stream connections); on `POST /v1/agent-signup`, `signin` (`RL_SIGNIN`, per client network). Has `Retry-After` |
| 429 | <a id="code-daily_cap_reached"></a>`daily_cap_reached` | yes | A daily cap; `details.cap` says which: `identity` or `tenant` (the send caps of tenant policy, the tenant one lowered by the new-workspace ramp), `invitations` (a tenant's 50 invitation emails a day), `shared_domain` (the deployment's shared-domain breaker, for the rest of the UTC day) or `billing_dispute` (the workspace's payment is disputed; sends stay stopped until the dispute closes). `details.resets_at` says when it resets, `null` for `billing_dispute` |
| 429 | <a id="code-agentic_budget_exhausted"></a>`agentic_budget_exhausted` | yes | The tenant's daily agentic-search budget is spent |
| 429 | <a id="code-upstream_rate_limited"></a>`upstream_rate_limited` | yes | A provider's rate limit stopped the request before anything changed. Cloudflare refused to create a zone with error 1105 (too many attempts to add a domain), for `nameservers` or `delegated_subdomain`: `Retry-After` and `details.retry_after` are `10800` (3 hours). Or the deployment's Amazon SES control-plane budget (one call per second, shared by every domain) had no slot within 5 seconds, for a domain create, `PATCH` or removal that calls SES: `Retry-After` is the wait, usually a few seconds |
| 429 | <a id="code-signup_limit_reached"></a>`signup_limit_reached` | yes | An agent sign-up budget is spent: `details.scope` is `network` (5 a UTC day), `asn` (100), `deployment` (`PM_AGENT_SIGNUP_DAILY_CAP`, or the sign-up mail share), `resend` (3 code emails an hour) or `human_email` (the address cannot take a sign-up today, for any reason concerning that person); `Retry-After` and `details.resets_at` |

A `402 billing_limit` is never returned for inbound mail (FR-BILL-8) or for replays of requests that already
completed (FR-BILL-6). On a deployment with billing `disabled`, only the daily caps in tenant policy apply,
and they return `429 daily_cap_reached` or `429 agentic_budget_exhausted`.

### Server and dependencies

| HTTP | Code | Retryable | When |
|---|---|---|---|
| 500 | <a id="code-internal_error"></a>`internal_error` | yes | A bug. Logged with `request_id` |
| 502 | <a id="code-upstream_error"></a>`upstream_error` | yes | A Cloudflare or SES API returned an unexpected error during a synchronous call (domain add, verify), or the SMTP relay could not be reached when a domain create (`smtp_relay`) or a `PATCH` with `smtp` tested it |
| 503 | <a id="code-unavailable"></a>`unavailable` | yes | A dependency is temporarily unavailable (D1, a Durable Object overloaded). `details.reason` names an operator state when there is one: `frozen` (a restore is in progress; `Retry-After: 300`), `read_only` (an automatic containment rule stopped writes from non-platform keys) or `storage_full` (the D1 database is full; nothing was written) |
| 503 | <a id="code-search_degraded"></a>`search_degraded` | yes | Only when the request set `"require_mode": true` and the requested mode is unavailable. Otherwise search degrades and sets `degraded: true` |
| 504 | <a id="code-timeout"></a>`timeout` | yes | An internal deadline was exceeded. For sends this happens **before** the message is queued, so a retry with the same key is safe |

### Retired codes

No code has been retired. A code removed in a later version keeps its anchor in a table here, so a `docs`
link in an old log still lands.

## Stream close codes

The WebSocket event stream (`GET /v1/events/stream`) ends with a close code. Before a close in the 4000 range
the stream sends an `error` message with the full envelope, because a close reason is limited to 123 bytes;
the reason is the code word below. The private codes are 4000 plus the matching HTTP status. The service
never sends 1005, 1006 or 1015; a client sees 1006 when a connection drops without a Close frame
([Event feed › Close codes](../project/design/event-feed.md#78-close-codes)). A client reconnects only on
`1001`, `1006`, `1011`, `1012`, `1013`, `4408` and codes it does not know, retries `4429` after 30 seconds,
reconciles after `4410`, and stops on every other code
([Event feed › Resuming without loss](../project/design/event-feed.md#710-resuming-without-loss)).

| Code | Reason | When | The client |
|---|---|---|---|
| 1000 | `normal` | The client closed the connection | – |
| 1001 | (set by the runtime) | A deploy or a runtime update: "Code updates disconnect all WebSockets" ([Durable Objects › WebSockets](https://developers.cloudflare.com/durable-objects/best-practices/websockets/)) | Resumes after 1–5 seconds, with jitter |
| 1006 | (seen, never sent) | The connection dropped | Resumes with backoff |
| 1008 | `policy_violation` | A binary or non-JSON frame, more than 100 messages in 10 seconds, or more than 10 errors | Stops and fixes the client; never reconnects on its own |
| 1009 | `message_too_big` | A client message over 16 KiB | Stops and fixes the client; never reconnects on its own |
| 1011 | `internal_error` | An unexpected failure in the object | Resumes with backoff |
| 1012 | `service_restart` | The service closes connections on purpose, for a change that needs fresh connections | Resumes after 5–30 seconds, with jitter |
| 1013 | `try_again_later` | Validation impossible for 180 seconds (D1 unavailable), or the object overloaded | Resumes after 30–60 seconds, with jitter |
| 4400 | `bad_request`, `subscribe_timeout` | An invalid `subscribe`, or none within 60 seconds | Stops and fixes its `subscribe`; never reconnects on its own |
| 4401 | `unauthenticated`, `key_revoked`, `key_expired` | A missing, used, expired or unknown stream token; or the key was revoked, expired or deleted | Gets a new key or token; never retries with the same |
| 4403 | `tenant_suspended`, `partner_suspended` | A suspension | Stops; a person must act |
| 4404 | `tenant_not_found`, `identity_not_found`, `consumer_deleted` | The tenant is being erased, the identity key's identity is deleted, or the subscribed consumer was deleted | Stops |
| 4408 | `slow_consumer` | The window stayed full for 60 seconds | Resumes from its last processed cursor, and sends acks |
| 4409 | `replaced` | The key opened an 11th connection, or another connection subscribed to the same consumer | Stops and reports it: a newer connection holds its place, and reconnecting would replace that one in turn |
| 4410 | `cursor_expired`, `log_reset` | The subscribe position or the consumer is behind the retained log, the connection fell behind it while open, or the log was rebuilt | Reconciles the gap, then resumes from `details.oldest_cursor` or resets its consumer |
| 4429 | `too_many_connections` | Token path only: the tenant already has 100 connections | Retries after 30 seconds |

## Send failures after `202`

Errors after a send was accepted do not come back as HTTP errors. They arrive as message status and
events (`message.rejected`, `message.failed`, `message.uncertain`, `message.bounced`). The reason codes
in `data.reason` are:

| Reason | Status | Meaning |
|---|---|---|
| `provider_validation` | `rejected` | The transport refused the content (header, size, format) |
| `sender_domain_unavailable` | `rejected` | The domain is not onboarded with the transport |
| `recipient_suppressed_by_provider` | per recipient `suppressed` | The provider's own suppression list. Synced into ours, except a suppression scoped to the shared platform domain, which may come from another workspace and is not copied |
| `quota_exhausted` | `failed` | The provider's rate limit or daily limit, still refusing after 24 hours of back-off (rate-limit back-offs are capped at 24 hours too), or an SMTP relay still unavailable after 24 hours of retries |
| `transport_timeout` | `uncertain` | No answer from the transport. It may have been sent |
| `transport_connection_lost` | `uncertain` | The connection dropped after the request was written |
| `provider_outcome_unknown` | `uncertain` | The transport reported a delivery failure for a message with several recipients without saying which recipients it refused, so some may have received it |
| `resolved_not_sent` | `failed` | A human resolved an uncertain send as not sent |
| `domain_failing_no_fallback` | `failed` | The domain failed and fallback was disabled by policy. A calendar message never falls back, whatever the policy |
| `marketing_needs_ses` | `rejected` | A `kind: marketing` message whose sending domain used the `cloudflare` transport when it reached the transport: the domain's transport changed, or fallback moved the message to the platform domain, after it was accepted. Cloudflare Email Service is for transactional mail only. At submit the same check returns `422 transport_unavailable` |

### How provider errors map to reasons

The transport's answer decides the reason. An answer that is not clearly definitive is treated as
unknown, because an `uncertain` send that a person or a later event settles is safer than a retry that
could send twice. `uncertain` sends are **never resent automatically**.

| Provider answer | Status | Reason | Retried |
|---|---|---|---|
| Cloudflare validation and header codes (`E_VALIDATION_ERROR`, `E_FIELD_MISSING`, `E_TOO_MANY_RECIPIENTS`, `E_CONTENT_TOO_LARGE`, `E_HEADER_*`, `E_HEADERS_*`, `E_TOO_MANY_ATTACHMENTS`, `E_RECIPIENT_NOT_ALLOWED`); SES `MessageRejected`, `BadRequestException` and other 4xx | `rejected` | `provider_validation` | never |
| Cloudflare `E_DELIVERY_FAILED` (the recipient server refused the message) on a message with one recipient | `rejected` | `provider_validation` | never |
| Cloudflare `E_DELIVERY_FAILED` on a message with several recipients (the error does not say which recipients were refused; spike S1 records the semantics) | `uncertain` | `provider_outcome_unknown` | never |
| A per-recipient provider event `failed` or `rejected` (Cloudflare event subscription, SES `Reject` or `Rendering Failure`) | that recipient `failed` or `rejected`; the message rolls up | `provider_validation` | never |
| Cloudflare `E_SENDER_DOMAIN_NOT_AVAILABLE`, `E_SENDER_NOT_VERIFIED`; SES `MailFromDomainNotVerifiedException`, `NotFoundException` | `rejected` | `sender_domain_unavailable` | never |
| Cloudflare `E_RECIPIENT_SUPPRESSED` | the recipient `suppressed` (or `rejected` when it cannot be identified) | `recipient_suppressed_by_provider` | the other recipients, once |
| Cloudflare `E_RATE_LIMIT_EXCEEDED`, `E_DAILY_LIMIT_EXCEEDED`; SES `TooManyRequestsException`, `LimitExceededException`, `SendingPausedException`, `AccountSuspendedException` | stays `queued` with back-off, then `failed` after 24 hours | `quota_exhausted` (at the end) | yes, it was definitely not sent |
| SMTP relay (`smtp_relay` domains): `535` or another `5xx` to `AUTH`, or no `STARTTLS` on port 587 (credentials never sent) | `rejected`; the domain gets issue `smtp_auth_failed` or `smtp_tls_required` | `sender_domain_unavailable` | never |
| SMTP relay: `5xx` to `MAIL FROM` or to the final `.`, or to every `RCPT TO` | `rejected` | `provider_validation` | never |
| SMTP relay: `5xx` to one `RCPT TO` | that recipient `rejected`; the others continue | `provider_validation` | never for that recipient |
| SMTP relay: any failure before the final `.` is written (connect, TLS, `4xx` to `AUTH`, `MAIL FROM` or every `RCPT TO`, a dropped connection), or `4xx` to the final `.`. A `4xx` to some `RCPT TO` keeps only those recipients `queued` | stays `queued` with back-off, then `failed` after 24 hours | `quota_exhausted` (at the end) | yes, it was definitely not sent |
| Cloudflare `E_INTERNAL_SERVER_ERROR`, any Cloudflare code not listed here, an error without a code; SES 5xx; a dropped connection; an SMTP connection lost after the final `.` and before its reply | `uncertain` | `transport_connection_lost` | **never** |
| No answer within 30 seconds; SMTP: no reply to the final `.` within 60 seconds | `uncertain` | `transport_timeout` | **never** |

The full table, with the exact Cloudflare and SES codes, is in
[Outbound design › Transport outcome classification](../project/design/outbound.md#transport-outcome-classification).

## Draft failures

A send that a draft makes without a caller waiting (a scheduled send, or an approval with `send: true`)
reports a refusal as `status: "failed"` and `failure.reason` on the draft, and in `draft.failed`. The
reasons are the error codes the send would have returned (`identity_paused`, `tenant_suspended`,
`partner_suspended`, `sending_paused`, `identity_owner_required`, `billing_limit`, `daily_cap_reached`,
`domain_not_ready`, `transport_unavailable`, `test_mode_recipient`, `loop_detected`,
`auto_reply_not_allowed`, `message_too_large`, `thread_not_found`, `source_unavailable`,
`identity_not_found`, `too_many_recipients`, `policy_denied`, `attachment_risky`, `calendar_invalid`,
`calendar_too_large`, `calendar_organizer_mismatch`, `calendar_attendee_mismatch`,
`calendar_recipient_mismatch`, `calendar_sequence_stale`, and the validation codes a later release could
refuse at send: `invalid_request`, `address_invalid`, `address_unsupported`, `header_not_allowed`,
`marketing_requirements_missing`), plus
`authority_revoked` (the key or person that scheduled it can no longer send), `sender_address_unavailable`
(its explicit `from_address` is no longer usable), `send_interrupted` (three attempts died before the
message was stored) and `send_window_missed` (not sent within an hour of `send_at`). A failed draft is never
retried automatically ([Drafts](../project/design/drafts.md)).
