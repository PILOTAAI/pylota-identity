# S9 Event subscriptions and onboarding APIs: result

**Pass, with the removal fallback taken**, 2026-10-27, in `pylotamail.com` (ADR 0023). The Worker `pi-spike-9`
(S1's spike Worker) logged every email it received and every message of the queue `pi-spike-9-events`.

## Event subscription

Created at runtime with the spike token through `POST /accounts/{account_id}/event_subscriptions/subscriptions`
(the call Wrangler 4.139.0 makes for `queues subscription create --source email.sending --zone-id --domain`).
Read back, it is:

```json
{ "name": "pi-spike-9-sub", "enabled": true,
  "source": { "name": "Email Service", "type": "email.sending",
              "zone_id": "…", "domain": "pi-spike-9.pylotamail.com" },
  "destination": { "type": "queues.queue", "queue_id": "…" },
  "events": ["message.delivered", "message.deferred", "message.bounced",
             "message.failed", "message.rejected", "message.complained"] }
```

Deleted with `DELETE …/event_subscriptions/subscriptions/{id}`. So `cloudflare_zone` and `nameservers` need
no `delivery_events: "manual"` fallback.

| Event type | Provoked by | Received |
|---|---|---|
| `delivered` | Mail to a routed address | Yes: `terminal: true`, the `payload` fields of [Outbound › Delivery events](../../docs/src/project/design/outbound.md#delivery-events) |
| `deferred` | Mail to an address whose Worker throws | Yes: `terminal: false`, `bounce: { type: "soft", classification: "temporary_failure" }`, `smtpResponse` `421 4.3.0 Upstream error …`, repeated on each retry |
| `bounced` | Mail to a name with no MX | Yes: `terminal: true`, `bounce: { type: "hard", classification: "permanent_failure", reason: "Permanent: no available upstream: …" }` |
| `rejected` | A suppression scoped to the sending domain (`POST /accounts/{a}/email/sending/suppressions` with `scope: { type: "sending_domain", value }`), with `drop_suppressed_recipients` off and on | **Not provoked**: both messages were delivered within a minute of adding the suppression. A hard bounce for a name without MX added no suppression either. M9 rechecks suppression timing before relying on it |
| `failed`, `complained` | – | Not provoked (an internal error, and a recipient's spam report) |

The three types received match the documented payloads; the parser keeps the documented shapes for the other
three, and an unknown `type` is acked and counted as designed.

## Onboarding

| Check | Result |
|---|---|
| Sending, subdomain: `POST /zones/{z}/email/sending/subdomains` `{ "name": "pi-spike-9.pylotamail.com" }` | Works (also in S1) |
| Sending, apex: the same call with `{ "name": "pylotamail.com" }` | Works: `return_path_domain: cf-bounce.pylotamail.com` |
| Routing, subdomain: `POST /zones/{z}/email/routing/dns` `{ "name": "r1.pylotamail.com" }` | Works (`ready`) |
| Routing, apex with `{ "name": "pylotamail.com" }` | **Refused**: `2007 Invalid Input: must be a subdomains of pylotamail.com`. With `{}` it enables the apex (three MX records, zone `ready`). [Identities and domains](../../docs/src/project/design/identity-domains.md) now sends `{}` for an apex |
| A literal rule whose `worker` action is the script name | Delivers (also S1, S2) |
| Apex catch-all to the Worker, then two subdomains enabled | The apex MX records and catch-all are unchanged |
| **The apex catch-all also takes subdomain mail**: `nobody@r2.pylotamail.com`, with no literal rule, reached the Worker | Cloudflare's page says catch-all rules exist only on the apex; in effect they also catch unmatched addresses of routed subdomains. The Worker's directory lookup already refuses unknown addresses, so nothing changes, but literal rules are not what keeps a subdomain's unknown addresses out |

## Removing one subdomain's routing

`PATCH /zones/{z}/email/routing/dns` `{ "name": "r1.pylotamail.com" }` unlocked `r1`'s MX and SPF records, and
deleting them by ID stopped `r1`'s mail (`bounced`, `no available upstream: r1.pylotamail.com`) while the apex
catch-all and `r2` kept receiving. The zone stayed `ready`.

But the subdomain stays registered. `GET /zones/{z}/email/routing/dns?subdomain=r1.pylotamail.com` still
answers with its records and `mx.missing` errors, and so do `pi-spike-1` and `pi-spike-2` from S1 and S2. It
keeps its place among the zone's 30 mail domains: with 26 sending domains and four such routing subdomains, the
27th sending domain was refused `2041 Maximum number of subdomains reached for this zone`. And
`DELETE /zones/{z}/email/routing/dns?subdomain=r1.pylotamail.com` ignores the parameter: it **disabled routing
for the whole zone**. **Fallback taken**: removal deletes the name's rules and records, and `idvy doctor`
reports the name under `routing.leftover_subdomains` with the dashboard step, which frees the place
([Identities and domains › Domain removal](../../docs/src/project/design/identity-domains.md#domain-removal)).

## Error codes

| Case | Result |
|---|---|
| 31st mail domain in a zone | `2041 Maximum number of subdomains reached for this zone` (leftover routing subdomains count) |
| `POST /zones` for a zone that exists | Not recorded: the spike token cannot create zones (`code 0`, `Requires permission "com.cloudflare.api.account.zone.create"`). Recorded with the first-run token in M13 |
| A zone deleted out of band | Not run: no zone could be spared. Recorded in M13 against a throwaway zone |
| Subdomain enable in a zone whose apex has a foreign MX | Not run: needs a second zone (ADR 0023). It runs against the Cloudflare fake until one exists |

Removing a sending domain leaves its `_dmarc` record, and disabling routing leaves the apex SPF record and the
`cf2024-1._domainkey` key; the first two were deleted by hand, the key cannot be (`1046 … managed by Email
Routing`).

## Cleanup

Removed on 2026-10-27: the Worker, the queue, the subscription, the suppression, the 26 sending domains (the
apex's included), `r1`'s and `r2`'s records, the leftover `_dmarc` and SPF records; zone routing is
`unconfigured`. **Owner step**: `r1`, `r2`, `pi-spike-1` and `pi-spike-2` are still registered routing
subdomains of `pylotamail.com`; remove them in the dashboard (Email Routing › Settings › Subdomains) so they
do not hold four of the zone's 30 places.
