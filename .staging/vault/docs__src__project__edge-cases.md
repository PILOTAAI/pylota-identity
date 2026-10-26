Where this goes: `docs/src/project/edge-cases.md`. Two changes: (1) a new section after section P (phone numbers), or after section Z if P is not yet pasted; (2) the preamble's counts and letters.

## 1. New section

## U · Vault

Rows of [Agent credential vault](design/vault.md) ([ADR 0021](adr/0021-agent-vault.md)). Every row is owned by
the service (`S`) or by the service and the integrator's consumer (`S+I`, tested by the consumer conformance
fixtures in `crates/conformance/vault`, `conf::vault::*`, and in the integrator's own suites). Tests named
`live::vault::*` run on staging ([Vault § 24](design/vault.md#24-tests)).

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| U1 | `PM_VAULT` off; the ops switch `vault_redeem` off | Every vault and consumer route `422 vault_disabled`; with the switch off, redemption, write-back and ticket minting refuse while reads and approvals continue | S | `it::vault::kill_switches` |
| U2 | `vault.allowed` false; set to true by a tenant or partner key; a Free-plan tenant on a Stripe deployment | `422 vault_disabled`; `403 permission_denied` (`platform_only_field`); `403 policy_denied` (`plan_required`) | S | `it::vault::gates` |
| U3 | Every response, idempotency record, event, audit row, log line and export after creating, rotating, importing and using items of each kind | No value, ticket, packet, unlock key or TOTP seed anywhere; the item shows `has` flags only | S | `it::vault::items_write_only` |
| U4 | A type holding a value that derives or implements `Debug` or `Serialize` | The build fails | S | `xtask::vault_types_have_no_debug` |
| U5 | A subject-access export of an identity with vault items | Item metadata only (kind, title, origins, health, dates); no value and no ciphertext | S | `it::vault::export_excludes_values` |
| U6 | Origins with `http`, userinfo, a path, a query, a fragment, a wildcard, upper case, a port below 1024, port 443 | Refused with `400 invalid_request` and the path, except case and port 443, which are canonicalised (lower case, port dropped) | S | `core::vault::origin::canonical_forms` |
| U7 | Origins that are IP literals in any notation, `localhost`, `.internal`, `.test`, the deployment's own hosts or the platform domain | `400 invalid_request` | S | `core::vault::origin::canonical_forms` |
| U8 | An `xn--` origin whose Unicode form skeleton-matches the ledger entry's service domain | `422 vault_kind_refused`, `details.reason = "confusable_origin"` | S | `core::vault::origin::canonical_forms` |
| U9 | A card number stored as a password, with spaces or dashes, 13–19 digits passing Luhn | `422 vault_kind_refused`, `details.reason = "payment_card_detected"`, on create and on rotation | S | `core::vault::refuse::cards_and_kinds` |
| U10 | Kinds `card`, `credit_card`, `bank_account`, `passkey`; a WebAuthn private key in any field | `422 vault_kind_refused` with the reason; no field accepts a WebAuthn key | S | `core::vault::refuse::cards_and_kinds` |
| U11 | A session state carrying `cf_clearance` and a name from `PM_VAULT_CLEARANCE_COOKIES`, on create, import and write-back | Dropped and counted in `clearance_cookies_dropped`; never refused | S | `core::vault::refuse::cards_and_kinds`, `it::vault::sessions` |
| U12 | An identity-scoped item without an approved ledger entry; an entry of another identity | `422 invalid_request` (`ledger_entry_not_approved`); `404 account_not_found` | S | `it::vault::ledger_binding` |
| U13 | An origin whose registrable domain is neither the entry's `service_domain` nor one of its `login_domains` | `422 invalid_request` (`origin_not_in_ledger`) at create; the same check at grant approval, use creation and redemption | S | `it::vault::ledger_binding` |
| U14 | The ledger entry is closed while a grant is active and a use is `in_use` | In the closure's batch: item archived, grants revoked, uses closed (`ledger_closed`); the next redemption `410 vault_ticket_expired` | S | `it::vault::ledger_binding` |
| U15 | An origin on `account.gov.uk`, or on a domain in `PM_VAULT_REFUSED_DOMAINS` | `422 vault_kind_refused` (`refused_domain`) at create and at every later check | S | `it::vault::ledger_binding` |
| U16 | A field ciphertext copied to another purpose, version, item or tenant; a DEK seal copied to another item | Opening fails (AAD mismatch); the redemption fails closed with `503 unavailable` and `vault_decrypt_failed` raised; nothing is returned | S | `core::crypto::pv1::aad_binding`, `core::crypto::hpke::rfc9180_vectors` |
| U17 | Two first writes of one tenant race to create its vault key | One key; the loser zeroises its secret and uses the winner's | S | `it::vault::tenant_key_race` |
| U18 | The tenant key is rotated with 450 versions sealed to it | New versions seal to the new key at once; the cron re-wraps 200 a run; the retiring key is deleted when none is left; `retiring_versions` reports progress | S | `it::vault::tenant_key_rotation` |
| U19 | A partner enrols custody with two slots; the request is replayed with its idempotency key; a tenant key asks | Two unlock keys shown once; the replay answers without them (`secret_replayed: false`); the tenant key `403 scope_denied` | S | `it::vault::custody` |
| U20 | The unlock header on a non-unwrapping route, from a tenant key, missing on a mint, wrong | `400 invalid_request` (`unlock_not_accepted`); the same; `403 vault_unlock_required`; `403 vault_unlock_invalid`, counted and rate-limited | S | `it::vault::custody` |
| U21 | A partner releases custody with a valid key; a partner loses both unlock keys | Back to operator custody; with both lost, every value is unrecoverable and the vault says so; no support path opens them | S | `it::vault::custody` |
| U22 | A tenant key approves a grant where people decide; a partner key under `vault.key_approval` without `on_behalf_of`; with it | `403 permission_denied` (`person_required`); `400 invalid_request` (path `on_behalf_of`); approved | S | `it::vault::grant_decisions` |
| U23 | The key that requested a grant (or created a use) approves it | `403 permission_denied` (`self_approval`) | S | `it::vault::grant_decisions` |
| U24 | A grant at its `expires_at` second; a pending grant at 7 days; the expiry job late | Uses refuse at the second (`grant_expired`); the pending one `expired` on read; the job only records | S | `it::vault::grant_decisions` |
| U25 | A use created by an identity key; without `Idempotency-Key`; replayed | `403 permission_denied` (the key cannot hold `vault:use`); `400 idempotency_key_required`; the use with `ticket: null`, `ticket_replayed: false` | S | `it::vault::use_lifecycle` |
| U26 | Item `per_run`, tenant floor `per_use`; a second use in the same run 30 and 61 minutes after an approval; `rotate_fill` on a `grant` item | Effective level is the strictest; within 60 minutes `approved_by = "run"`, after it a person; `rotate_fill` always `per_use` | S | `core::vault::policy::approval_needed`, `it::vault::use_lifecycle` |
| U27 | Approval with a digest that is not the use's | `400 invalid_request` (path `digest`); the use stays pending | S | `core::vault::digest::stable`, `it::vault::use_lifecycle` |
| U28 | A pending use past `deadline_at`; then an approval | `expired` by the cron or on read; `409 vault_not_pending` | S | `it::vault::use_lifecycle` |
| U29 | A ticket first redeemed 61 seconds after minting | `410 vault_ticket_expired` (`not_redeemed`); the use expires | S | `it::vault::use_lifecycle` |
| U30 | A spent phase redeemed again; a lost redemption response | `409 vault_phase_spent`; the orchestrator creates a new use, covered by a `per_run` approval | S | `it::vault::use_lifecycle` |
| U31 | A ticket minted twice; the first presented | The first stops matching at once: `404 vault_ticket_not_found` | S | `it::vault::use_lifecycle` |
| U32 | Each cause of §7.4 while a use is `in_use` | The use closes in the cause's batch with its `close_reason`; ticket hash and sealed DEK cleared; switches read live before the batch | S | `it::vault::use_lifecycle` |
| U33 | Consumer requests with no signature, two signatures, a wrong tag, wrong components, `created` 31 seconds off, `expires` past, a body that does not match `Content-Digest`, a wrong key, an `Authorization` header | One byte-identical `401 consumer_unauthenticated` body (but `request_id`), counted in the failed-authentication bucket | S | `it::vault::consumer_signatures` |
| U34 | A nonce reused within its window | `401 consumer_unauthenticated` | S | `it::vault::consumer_signatures` |
| U35 | A consumer `pending_activation`, `suspended`, `revoked`; its partner suspended | `403 consumer_not_active`; the partner's suspension leaves the consumer's status unchanged | S | `it::vault::consumer_reach` |
| U36 | A consumer rotates keys with a 3,600-second overlap; signs with the old key at 3,599 and 3,601 seconds; redeems | Accepted, then `401`; the packet is sealed to the new encryption key named in `enc_kid` | S | `it::vault::consumer_signatures` |
| U37 | Redemption context with another `run_id`, top-level origin, frame origin, trigger or form kind | `403 vault_refused` with the reason; the use closes (`refused`) | S | `it::vault::redemption_binding` |
| U38 | Context with recording, logging or CAPTCHA solving on, extensions present, another hosting or provider, an undeclared mode | `403 vault_refused` with the reason; the use closes | S | `it::vault::redemption_binding` |
| U39 | The item rotated between minting and redemption | `410 vault_ticket_expired` (`item_rotated`) | S | `it::vault::redemption_binding` |
| U40 | Registration with a JWK carrying `d`, a `contract` other than `vault-consumer/1`, an eleventh platform-level consumer | `400 invalid_request` (`private_key_supplied`, `contract_unsupported`); `422 vault_consumer_limit_reached` | S | `it::vault::consumer_reach` |
| U41 | Two uses of one login redeem `totp` in the same time step | The second `409 vault_totp_step_used` with `details.retry_after`; the phase stays open | S | `it::vault::totp_step_once` |
| U42 | A `totp` redemption with 5 seconds left in the step | The next step's code with `not_before`; the consumer waits at most 7 seconds | S+I | `core::vault::totp::rfc6238_vectors`, `conf::vault::totp_not_before` |
| U43 | A verification email for a vault login's channel; read by an agent key, by a key with `quarantine:review`, in the console | Hidden from every agent surface and event; the key sees the row with `content_withheld: "vault_code"`; `raw` is `404`; a signed-in person sees it | S | `it::vault::email_codes_rule_4b` |
| U44 | A code that arrives with no open `email_otp` phase; then a phase opens 2 minutes later | It stays hidden and is delivered to the phase if it arrived after `use.created_at − 60 s`; otherwise it expires unused after 24 hours | S | `it::vault::email_codes_rule_4b` |
| U45 | `vault.codes_into_fills_only` on; `wait` with `kind=verification`; a code from a receive-allowed sender | `403 policy_denied` (`codes_into_fills_only`); the allowed sender's code is released as today | S | `it::vault::email_codes_rule_4b` |
| U46 | A ledger entry without a vault item, strict mode off | Its codes reach `wait` as today; nothing is withheld | S | `it::vault::email_codes_rule_4b` |
| U47 | An SMS code from a sender not in `otp_channel.senders`; for a use created 11 minutes earlier; unsolicited | Not taken; the phase times out | S | `it::vault::sms_otp_phase` |
| U48 | A use naming `sms_otp` before the phone milestone ships | `400 invalid_request` (`sms_not_available`) | S | `it::vault::sms_otp_phase` |
| U49 | A session over 1,048,576 bytes, 500 cookies or 10 origins after filtering | `422 vault_session_too_large` with `details.limit` | S | `it::vault::sessions` |
| U50 | An import with `source = "pylota_sealed_session"`, repeated with its idempotency key; carrying third-party cookies | Filtered, counted, audited with `source_ref`; the replay returns the same item; no second item | S | `it::vault::session_import` |
| U51 | A second `session_load` while one is `in_use` | `403 vault_refused` (`session_in_use`) | S | `it::vault::sessions` |
| U52 | A write-back; then a version past `expires_at` | Version `n + 1` current, `n`'s row and object deleted, other uses closed; the expired version deleted and the item `needs_auth` | S | `it::vault::sessions` |
| U53 | A login rotated with a 24-hour overlap; a use with `version: "previous"` at 23 and 25 hours | Uses close at rotation; the previous version works until the overlap ends, then is deleted | S | `it::vault::rotation_versions` |
| U54 | `rotate_fill` reported `password_changed` with write-back; `password_change_failed`; no confirmation by `closes_at` | New version current; pending deleted; pending kept 24 hours with `rotation_unconfirmed` for a person to settle | S | `it::vault::rotation_versions` |
| U55 | An executor writes back a rotated API key on an item with `rotate_after_use`; a second write-back | New current version, previous kept for its overlap; `409 vault_phase_spent` | S | `it::vault::rotation_versions` |
| U56 | `sign` with HMAC-SHA256 and SigV4 against published vectors; a SigV4 `url` off the item's origins; a `timestamp` 6 minutes old | Signatures match the vectors; `400 invalid_request` for the other two | S | `it::vault::signing` |
| U57 | A console reveal without re-authentication, the eleventh in a day, of a TOTP seed, of a session | Re-authentication asked; refused with the daily limit; only the current code; not offered | S | `browser::vault::reveal_rules` |
| U58 | Any API, MCP or CLI attempt to reveal, from any key level | No such route or tool exists; `404 route_not_found` for guessed paths | S | `it::vault::mcp_tools`, `cli::vault::commands` |
| U59 | An API key writes, grants or uses a `managed_secret` item | `422 invalid_request` (`managed_item`); listing shows metadata and `managed_by` | S | `it::vault::managed_secrets` |
| U60 | "Show a new value" for a port-out PIN in partner custody | The owning service sets a new PIN at the provider, the vault seals it and shows it once with `no-store`; audit `item.revealed` with `rotated: true` | S | `it::vault::custody` |
| U61 | 50 concurrent writers to one tenant's vault | No gap and no duplicate `seq`; every change commits with its row or not at all; three conflicts answer `503 unavailable` | S | `it::vault::audit_chain_and_age_only`, `live::vault::d1_audit_contention` |
| U62 | A refusal whose audit batch fails | The refusal is still answered; `vault_audit_append_failures_total` counts and pages | S | `it::vault::audit_chain_and_age_only` |
| U63 | Retention deletes a tenant's oldest audit rows; a reader verifies from the anchor; a row edited in D1 | The chain verifies from the anchor; the edited row fails verification at its `seq` | S | `core::vault::audit::row_hash_chain`, `it::vault::audit_chain_and_age_only` |
| U64 | `vault.allowed` set false with uses `in_use` | Every open use closes (`kill_switch`); redemptions refuse at once, before the batch completes | S | `it::vault::kill_switches` |
| U65 | `revoke-all` with `shred_sessions` and `archive_items` over 2,500 rows | Every grant revoked, use closed, session version deleted and item archived, in batches of 1,000; one `vault.revoke_all` row | S | `it::vault::kill_switches` |
| U66 | Deleting an item; its R2 delete fails | Rows shredded in one batch with the receipt; the daily sweep deletes the object | S | `it::vault::erasure_scopes` |
| U67 | Identity erasure with identity-scoped items and grants on tenant-scoped items | Its items shredded, its grants deleted and uses closed, audit rows kept with `run_id` cleared; receipt counts | S | `it::vault::erasure_scopes` |
| U68 | Tenant erasure in partner custody | Custody slots and tenant keys deleted first, then the rest; the probe finds no row and an empty `t/{tenant_id}/vault/` prefix | S | `it::vault::erasure_scopes` |
| U69 | A sentence in `docs/` or `site/` claiming the vault is zero-knowledge or end-to-end encrypted without a negation in the sentence; "we cannot see your passwords" outside double quotes; the trust statement, which denies both | The build fails on the first two; the trust statement passes | S | `xtask::docs_forbidden_claims` |
| U70 | `PM_VAULT` turned off while `vault_code` mail and SMS are hidden | They stay hidden and expire; nothing is released | S | `it::vault::email_codes_rule_4b` |
| U71 | MCP `tools/list` with each vault permission; each tool's result | Tools listed only by permission; no value, ticket, packet or placeholder in any result; no approving tool | S | `it::vault::mcp_tools` |
| U72 | `idvy vault items create` with a value as an argument; any command's output | Values only from standard input or a file; never printed; an unlock key shown once | S | `cli::vault::commands` |
| U73 | The browser types a use's placeholder into a second form on the ticket's origin (a comment box) and submits | The proxy drops the request and reports `placeholder_misplaced`; nothing substituted; the use closes | S+I | `conf::vault::proxy_second_form` |
| U74 | A request carrying a placeholder to `http://` on the ticket's host | Dropped and reported; no value on the wire | S+I | `conf::vault::proxy_http` |
| U75 | A placeholder in a header, in the URL, in a compressed body, in a body whose path differs from the recorded form `action` | Dropped and reported `placeholder_misplaced` | S+I | `conf::vault::proxy_header_url_websocket`, `conf::vault::proxy_second_form` |
| U76 | The upstream echoes the value gzip-compressed, split across two chunks, base64url-encoded, or in `Location` | Redacted before the browser sees it and `value_exposed` reported, or the response dropped with `value_exposed_unscannable`; the item `needs_rotation` | S+I | `conf::vault::echo_gzip`, `conf::vault::echo_split_chunk`, `conf::vault::echo_location` |
| U77 | A placeholder in a WebSocket text, fragmented or compressed frame, in either direction | The socket closes; `placeholder_misplaced`; never substituted | S+I | `conf::vault::proxy_header_url_websocket` |
| U78 | An item gains an origin while a grant on it is active | The grant's origins are unchanged; a use at the new origin `403 vault_not_granted` (`origin_not_granted`) until a new grant | S | `it::vault::grant_decisions` |
| U79 | A key of any level mints an identity key listing `vault:use`, `vault:approve` or `vault:admin` | `400 invalid_request` (`permission_not_allowed_for_level`); no identity key can ever approve or start a use | S | `it::vault::identity_key_levels` |
| U80 | An executor's request carrying a secret receives a `307` to another host | Not followed; reported as a failure; the secret is not re-sent | S+I | `conf::vault::proxy_redirect_307` |
| U81 | A consumer fixture or a platform fetch resolves to `64:ff9b::a9fe:a9fe`, `2002:a9fe:a9fe::1` or `169.254.169.254` | Refused by the embedded IPv4 rule of [Security § 9.1](design/security.md#91-url-rules-at-create-update-and-every-attempt) | S | `it::security::ssrf_embedded_ipv4` |
| U82 | A grant is revoked while the egress proxy holds an open tunnel and a WebSocket carrying its secret | The status route answers `closed` at once; within 30 seconds both connections close and the secret is discarded | S+I | `it::vault::use_status_closure`, `conf::vault::closure_mid_run` |
| U83 | A tenant generates 100,000 refusals in an hour | Rate limited at `RL_VAULT`; no audit row is deleted by count; only rows past 400 days ever go | S | `it::vault::audit_chain_and_age_only` |
| U84 | A grant's or use's `purpose` holds a URL, Markdown link syntax and HTML | Shown as escaped text on every approval surface and in notification email; never a link, image or markup | S | `browser::vault::approval_no_links`, `it::vault::grant_decisions` |
| U85 | Redemption context with `top_level_origin` `HTTPS://TFL.GOV.UK.:443` for a use at `https://tfl.gov.uk`; a consumer comparing its own way | Canonicalised to the same origin and accepted; consumers use `canonicalise_origin` from the SDK | S+I | `core::vault::origin::canonical_forms`, `conf::vault::canonical_origin` |

## 2. Preamble

Add 85 rows in one section, all owned by the service (`S`, or `S+I` where the consumer conformance fixtures test
the integrator's part), and add section U to the list of sections that follow W. Section U was allocated to
this design by the coordinator's brief although the preamble says sections skip U because of the PRD's selling
points U1–U10: every other letter is taken or avoided (M milestones, S spikes; P now holds phone rows). If the
coordinator keeps U, add after the sentence about P: "Section U holds vault rows, always written with the
section letter and a number from 1 to 85, and the PRD's selling points are always called 'selling point U1' in
prose; a vault row is 'row U1'." Otherwise rename the rows here and in `design/vault.md` in one change.

Add `conf::vault::*` to the description of `conf::` tests: "and the vault consumer fixtures in
`crates/conformance/vault`, run by `idvy vault conformance` against a fixture server".
