Where this goes: `docs/src/reference/limits.md`, a new section after "## Agent self-sign-up" (before "## Deliverability metrics").

## Phone numbers

From [Phone numbers](../project/design/phone-numbers.md). Telnyx's own limits that shape these (messaging webhooks
acknowledged within 2 seconds; 0.1 messages a second per long code and 20 per toll-free number, queued above that;
30 dials a second per region; documents of at most 20 MB, linked within 30 minutes; number reservations of 30
minutes) were read from Telnyx's documentation on 2026-10-24 and can change.

| Limit | Value |
|---|---|
| Numbers per identity, per tenant | 5 and 20 by default (policy); at most 20 and 200 |
| Requests per tenant | 20 `pending_approval` (blocked ones included); 10 running (`executing`, `in_progress`, `action_needed`, `uncertain`) |
| Blocked request lifetime | 7 days after it is unblocked, at most 30 days after creation |
| Standing budget | `max_numbers_per_identity` 1–20; `max_new_numbers_per_month` 1–200 |
| One-call purchase | Inline run up to 15 seconds; up to 5 candidates when a number is taken; `details.alternatives` up to 5 |
| `expires_at` | 1 hour to 365 days after the purchase |
| Onboarding drafts | One per tenant and country; 2–5 sample messages of 20–1,024 characters; other text fields 1–1,000 characters; deleted after 30 days untouched |
| Sole-proprietor one-time code | 5 resends a day |
| Pending request lifetime | 7 days, then `rejected` (`expired`) |
| Quote token | 30 minutes |
| Searches | 300 a day per tenant; `limit` 1–20 per search |
| SMS text | 1–10 segments (GSM-7 153 characters a segment when split, UCS-2 67) |
| SMS caps | 200 segments a day per identity, 2,000 per tenant (policy) |
| Inbound SMS | 30 an hour per sender per identity, 600 an hour per identity (policy); text kept up to 10,000 characters |
| MMS media | 10 items, 5 MB each |
| `sms/wait` timeout | 1–60 seconds |
| Verification values | Released for 24 hours after arrival; repeatable for 1 hour after first release; vault deposits at most 15 minutes |
| KYC documents | PDF, PNG or JPEG; 1–20,971,520 bytes; 50 per tenant; upload URL one hour |
| Profile addresses | 20 |
| `sms_senders` per ledger entry | 10 |
| Voice connectors per tenant | 5 |
| Outbound call length | `phone.calls.max_minutes`, default 15, at most 120 |
| Port-out response | `respond_by` 24 hours after the request |
| Tenant reconcile | Once every 10 minutes |
| Webhook body | 256 KB |

Provider-side backstops: each tenant's messaging profile and outbound voice profile carry a daily spend limit in USD
of 1.5 times the tenant's daily cap at its highest price; SMS above a number's sending rate wait in `queued` rather
than fill the provider's queue.
