# S7 Outbound Message-ID: result

**Pass: strategy A**, 2026-10-27, from the sends of spike S1 ([`spikes/s1-bindings/RESULT.md`](../s1-bindings/RESULT.md)).

S1 sent from `s1@pi-spike-1.pylotamail.com` through the `send_email` binding to addresses routed back to the
same Worker, so each received copy's headers could be compared with what `send()` returned.

| Send | `send()` returned `messageId` | Received `Message-ID` |
|---|---|---|
| Structured, one recipient | `<BjpILarfgnpagEBJyWhbvPShPmm988BzTEYp@pi-spike-1.pylotamail.com>` | identical |
| Structured, two recipients | `<dnr1Va01nmSjDu9jrEklrfLIBF9ICwIz8nro@pi-spike-1.pylotamail.com>` | identical in both copies |
| Raw `EmailMessage`, composed `Message-ID: <s1-cal-each-…@pi-spike-1.pylotamail.com>` | `<TNGQyKzuXJvMRhJE2DI8W3JvztNjAOtaB4LQ@pi-spike-1.pylotamail.com>` | the returned value; the composed one was replaced |

The mapping is the identity: the returned value, angle brackets included, is the header. Its domain is the
sending domain, not a Cloudflare domain. [Outbound › Message-ID](../../docs/src/project/design/outbound.md#message-id-of-outbound-mail-spike-s7)
now takes strategy A; the journal copy (strategy B) is not built.
