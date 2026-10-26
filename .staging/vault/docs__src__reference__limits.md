Where this goes: `docs/src/reference/limits.md`, a new section after the phone section "## Phone numbers" (or after "## Agent self-sign-up", before "## Deliverability metrics").

## Vault

From [Vault § 19](../project/design/vault.md#19-billing-and-limits).

| Limit | Value |
|---|---|
| Items | 100 per identity; `vault.max_items` per tenant, default 1,000, at most 10,000 |
| Origins | 1–10 per item and per grant; `login_domains` at most 5 per ledger entry |
| Values | Password 1–1,024 bytes; secret 1–16,384 bytes; recovery codes 1–20 of at most 64 characters; TOTP secret 16–128 base32 characters, `digits` 6 or 8, `period` 15–120 seconds |
| Generated passwords | 16–128 characters, default 24 with all four classes |
| Session state | 1,048,576 bytes after filtering, 500 cookies, 10 origins; TTL 1 hour to `vault.session_max_ttl_days` (default 30) |
| Grants | 20 pending per identity; 50 not ended per item; pending 7 days; active 1 hour to 90 days, default 7 days; `max_uses` 1–100,000 |
| Uses | 120 a minute per identity; 100 waiting for approval per tenant; approval deadline 60–900 seconds, default 600 |
| Tickets | First redemption within 60 seconds of minting; a window of 10 minutes for `fill` and `rotate_fill`, 300–7,200 seconds (default 1,800) for `session_load`, `session_capture` and `inject` |
| Code long poll | `wait_seconds` 0–60 |
| Consumers | 10 platform-level, 10 per partner, 5 per tenant (not counting revoked) |
| Consumer requests | Body 1,572,864 bytes; signature `created` within 30 seconds of the server clock, `expires` at most 60 seconds after `created`; use status 6 a minute per use |
| Key rotation overlap for consumers | 0–86,400 seconds, default 3,600 |
| Version overlap | 0–168 hours, default 24 |
| Reveal | 10 a person a UTC day, after signing in again within 10 minutes |
| Signing (P1) | `message_base64` 1–65,536 bytes; SigV4 `headers` at most 50, `timestamp` within 5 minutes |
| `title` | 1–120 characters; `purpose` 1–500 (grants) or 1–200 (uses); `run_id` 1–128 |
| Audit | Kept 400 days, by age only |
