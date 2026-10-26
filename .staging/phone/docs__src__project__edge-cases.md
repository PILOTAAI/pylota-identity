Where this goes: `docs/src/project/edge-cases.md`. Two changes: (1) a new section after the last section, "## Z · Parity additions" (after its last row); (2) the preamble's counts.

## 1. New section, after the end of section Z

## P · Phone numbers

Rows of [Phone numbers](design/phone-numbers.md) ([ADR 0020](adr/0020-phone-numbers.md)). P107 is
integrator-owned and tested in Pylota's own suites. Tests named `live::phone::*` run against the real provider
on staging ([Phone numbers § 35](design/phone-numbers.md#35-tests)).

| # | Case | Required behaviour | Owner | Test |
|---|---|---|---|---|
| P1 | Search on an offered row | Candidates carry every capability of the row (a GB mobile without `sms` is never returned), a GBP quote and a 30-minute quote token; only the requested number type | S | `core::phone::catalogue::rows_valid`, `it::phone::search_quotes_and_features` |
| P2 | A candidate whose provider cost exceeds the row's ceiling (vanity, premium) | Dropped from results; never orderable | S | `it::phone::search_drops_premium` |
| P3 | Search or request for a `not_offered` row, or a `requires_local_presence` row without the presence | `422 country_not_offered`; `422 local_presence_required` with `details.local_presence`; nothing stored | S | `core::phone::catalogue::eligibility`, `it::phone::catalogue_refusals` |
| P4 | `phone.allowed` false; a Free-plan tenant on a Stripe deployment; `PM_PHONE` off | `403 policy_denied` (`phone_not_allowed`, `plan_required`); with the flag off every phone route `404 route_not_found` | S | `it::phone::gates` |
| P5 | A purchase for a row needing an approved profile, without one | `409 compliance_not_approved` at creation and again at execution; no provider call | S | `it::phone::compliance_required` |
| P6 | A profile with an individual or sole-trader entity type | `422 compliance_incomplete`, `details.reason = "entity_type"` | S | `core::phone::requests::profile_validate` |
| P7 | Document: wrong content type, over 20,971,520 bytes, checksum mismatch, executable or macro sniffed | `400 invalid_request`, `400 upload_size_mismatch`, `400 upload_checksum_mismatch`, `422 attachment_risky`; nothing stored | S | `it::phone::document_upload_rules` |
| P8 | Reading a document | Metadata only on every route; no route returns bytes; the R2 object is ciphertext under a per-document sealed key | S | `it::phone::documents_write_only` |
| P9 | Deleting a document a live provider group needs | `409 phone_document_in_use`; deletion of an unused one shreds the key, then deletes the object | S | `it::phone::document_delete` |
| P10 | A submission run that cannot link documents within 20 minutes of the first upload | Uploaded provider documents deleted once unlinked; the request `failed` (`upstream_error`); no document left unlinked at the provider | S | `it::phone::submission_link_window` |
| P11 | A provider requirement type with no mapping | `422 compliance_incomplete` with `details.missing[]`; `phone_requirement_unmapped` raised | S | `core::phone::catalogue::map_requirements`, `it::phone::requirement_unmapped` |
| P12 | Provider address validation fails | Request `failed` (`compliance_address_invalid`) with `details.suggested`; no address stored unvalidated for a number that can call | S | `it::phone::address_validation` |
| P13 | A requirement group declined | Profile row `rejected` with the comments as escaped untrusted text; owners notified; a new request resubmits | S | `it::phone::group_declined` |
| P14 | A group `expired`, `no-longer-eligible`, or an unknown status | `expired`; an unknown value keeps the status and raises `phone_status_unknown` | S | `core::phone::telnyx::group_status_table` |
| P15 | Request without `Idempotency-Key`; repeated with the same key and body; same key, other body | `400 idempotency_key_required`; the stored response with `Idempotent-Replayed: true`; `409 idempotency_conflict` | S | `it::phone::request_idempotency` |
| P16 | A tenant key approves where people decide; a partner key under `phone.key_approval`; a tenant key with `phone.key_approval` | `403 permission_denied` (`person_required`); allowed; still `person_required` | S | `it::phone::approval_who` |
| P17 | A key approves a request it created | `403 permission_denied` (`self_approval`) | S | `it::phone::approval_who` |
| P18 | Approval naming a stale digest; approval of a request not `pending_approval` | `409 phone_request_digest_mismatch` with `details.current_digest`; `409 phone_request_not_pending` | S | `it::phone::approval_binding` |
| P19 | A pending request past 7 days | `rejected` (`expired`) by the cron with its event; a late approval gets `409 phone_request_not_pending` | S | `it::phone::request_expiry` |
| P20 | A higher price at execution than quoted; a lower one | Back to `pending_approval` with a new quote and digest (`reason: "quote_changed"`), approval void; the lower price is charged | S | `it::phone::quote_changed` |
| P21 | Two approvals racing, a redelivered `execute`, a cron sweep, and a run whose lease lapsed | One run at a time; the stale run's guarded writes change nothing and it stops | S | `core::phone::requests::claim_rules`, `it::phone::claim_and_fencing` |
| P22 | The 21st pending request of a tenant, or the 11th running | `422 phone_request_limit_reached` | S | `it::phone::request_limits` |
| P23 | Two tenants order the same number at once | The second fails at the directory's unique live index before any provider call; no spend | S | `it::phone::same_number_two_tenants` |
| P24 | The number is taken between search and order (85001) | Listing proves no order; spend intent cleared; hold released; number `failed` (`no_numbers_available`); request `failed` | S | `it::phone::number_taken` |
| P25 | A definitive refusal whose reference listing is incomplete or fails | The attempt is `uncertain`; nothing cleared by guess | S | `core::phone::orders::absence_proved`, `it::phone::refusal_needs_listing` |
| P26 | The order call times out and the order exists | `uncertain`; the reconciler finds it by reference and adopts it; exactly one order | S | `it::phone::uncertain_found` |
| P27 | The order call times out and no order exists; or nothing can be proved for 24 hours | Two complete empty listings (≥ 5 min, then ≥ 10 min) prove absence; the next attempt uses a new reference (at most 3); otherwise `carrier_order_uncertain`, `phone_order_uncertain_stuck`, and only the platform resolve ends it | S | `core::phone::orders::absence_proved`, `it::phone::uncertain_absent_and_stuck` |
| P28 | Requirements pending on an order | `requirements_pending`, `phone_number.action_needed`, notifications; reminders at 72 and 24 hours before `deadline_at` | S | `it::phone::requirements_pending` |
| P29 | The deadline passes and the provider cancels | Number `failed` (`regulatory_rejected`, `deadline_passed`) after a complete listing shows no live number; spend intent cleared; the next request orders | S | `it::phone::deadline_cancel_not_stranded` |
| P30 | A review lasting 7 days | Checks at 1 h, 6 h, then daily; `phone_review_long` at 7 days; no loop and no `provision_in_progress` | S | `it::phone::long_review_backoff` |
| P31 | Provider `402` or code 20100 on an order | `carrier_billing`; page alert; nothing ordered | S | `it::phone::provider_billing` |
| P32 | Provider `401` or `403` | `not_configured`; `phone_provider_auth`; the ops switch moves to `paused_purchases` | S | `it::phone::provider_auth` |
| P33 | `PM_TELNYX_API_KEY` missing | `not_configured` before any provider call; not retried | S | `it::phone::not_configured` |
| P34 | Carrier active but the messaging read-back differs | Retried; then `sms_status = failed` (`sms_configuration`); the number stays `configuring`; voice unaffected | S | `it::phone::facets_independent` |
| P35 | Deleting an identity that holds a live number | `409 identity_has_phone_numbers` | S | `it::phone::identity_delete_refused` |
| P36 | A paused identity | Inbound SMS stored and evented; sends `409 identity_paused`; requests execute | S | `it::phone::identity_paused` |
| P37 | The tenant is suspended during a run | The run's next guarded write fails and it stops; sends `403 tenant_suspended` | S | `it::phone::tenant_suspended_mid_run` |
| P38 | Release of a live number without the ownership stamp; a truncated listing; a pending order; a `404` | Refused with `phone_release_unattributable`; never read as absent; waits; success. The voice route is removed before the number | S | `it::phone::release_safety` |
| P39 | A number released by tenant A offered to tenant B within 90 days | Excluded from B's search and refused at execution; A may buy it back | S | `it::phone::release_quarantine` |
| P40 | Moving a number to another identity of the tenant, and to an identity of another tenant | Moved, history kept with the old identity; `404 identity_not_found` | S | `it::phone::move_number` |
| P41 | A crash after creating the tenant's messaging profile and before recording it | The next run finds it by name and tag and adopts it; no second profile | S | `it::phone::tenant_resources_adopted` |
| P42 | A webhook with a bad or missing signature, or a timestamp outside 5 minutes, in a test tenant or staging | `403 invalid_signature`; nothing staged | S | `core::phone::telnyx::verify_signature`, `it::phone::hook_signature` |
| P43 | `PM_TELNYX_PUBLIC_KEY` unset | `503 unavailable` (`not_configured`); nothing processed | S | `it::phone::hook_signature` |
| P44 | R2 or the queue fails in the hook; D1 is down | `503` (the provider retries); with D1 down the hook still answers `200` | S | `it::phone::hook_staging_failures` |
| P45 | The same event delivered twice | One effect | S | `it::phone::hook_dedup` |
| P46 | An event whose second fact disagrees (profile or connection of another tenant) | No effect; `ignored_mismatch`; `phone_event_mismatch` | S | `it::phone::event_cross_check` |
| P47 | A late `message.sent` after `delivered` | Ignored by `occurred_at` | S | `it::phone::event_ordering` |
| P48 | SMS to an unknown, released, ported-out or failed number | `ignored_unrouted`; nothing stored | S | `it::phone::sms_unrouted` |
| P49 | An inbound SMS | Stored with `sender_unverified`; text untrusted; never refused for plan or spend | S | `it::phone::sms_inbound_stored` |
| P50 | STOP, START or HELP from a blocked or throttled sender | The consent effect applies anyway; no confirmation from Pylota Identity | S | `it::phone::keywords_always_apply` |
| P51 | Screening in `hold`: decisive scan; undecided; short or keyword message; classifier unavailable with `on_unavailable = quarantine` | `quarantined` (`steering_suspected`); `screening` then a verdict; skips the model; `quarantined` | S | `it::phone::sms_screening` |
| P52 | A sender over 30 an hour, or an identity over 600 | `throttled`; not evented | S | `it::phone::sms_inbound_caps` |
| P53 | Counterparty erasure of an E.164 | Its SMS, media and calls deleted in every identity of the tenant; the opt-out kept as a hash; the next send refused | S | `it::phone::counterparty_erasure` |
| P54 | MMS with more than 10 items, an item over 5 MB, a host not on the list, a risky item | Items beyond the limit `unavailable`; foreign hosts never fetched; a risky item quarantines the SMS (`risky_attachment`) | S | `it::phone::mms_media` |
| P55 | A STOP to tenant A's number | Tenant B's sends to the same person are unaffected at both layers | S | `it::phone::stop_isolated` |
| P56 | SMS in a test tenant | Simulator only; no provider call; simulated IDs stay in the test tenant | S | `it::phone::test_tenant_simulator` |
| P57 | A solicited code from an approved entry's listed sender | Released by `sms/wait`; repeat within 1 hour returns the same value; then purged | S | `it::phone::sms_wait_release` |
| P58 | A code with `accounts.require_approval` and no matching entry; `sms/wait` without `account_id` | `quarantined` (`account_unapproved`); `403 policy_denied` (`account_not_approved`) | S | `it::phone::sms_ledger_gate` |
| P59 | A code SMS on a number named by an active vault login item's phone channel | `hidden` (`vault_code`); no events; withheld from keys' reads; taken once by `SmsCodeSource::take`; deleted after 24 hours | S | `it::phone::vault_code_withheld`, `it::vault::sms_otp_phase` |
| P60 | `vault.codes_into_fills_only` true | Every code SMS withheld except receive-allowed senders; `sms/wait` `kind=verification` gets `403 policy_denied` (`codes_into_fills_only`) | S | `it::phone::vault_strict_mode` |
| P61 | Reads, events and logs of a code-bearing SMS | Text shows `[code]`; the canary code appears in no log line | S | `it::phone::code_redaction` |
| P62 | An unsolicited code with `quarantine.unsolicited_otp`; a link in an SMS | `quarantined` (`otp_unsolicited`); a link is never a verification object | S | `core::phone::verify::find_code`, `it::phone::sms_unsolicited` |
| P63 | Unknown outcome on `POST /messages` | Repeats with the same provider key (only once the contract test passed) send one SMS; then `uncertain`, never resent | S | `live::phone::send_idempotency`, `it::phone::sms_unknown_outcome` |
| P64 | Send repeated with the same key and body; other body; in progress | Stored response; `409 idempotency_conflict`; `409 request_in_progress` | S | `it::phone::sms_idempotency` |
| P65 | `from` of another identity, a platform number, a released number; a number still configuring | `404 phone_number_not_found`; `409 phone_number_not_ready` | S | `it::phone::sms_sender_binding` |
| P66 | A receipt before the provider ID is stored | Held `pending_target` up to 15 minutes, then applied | S | `it::phone::receipt_race` |
| P67 | An opted-out recipient; marketing without opt-in; provider 40300 | `403 recipient_opted_out`; `403 consent_required`; ledger entry `provider_sync` | S | `it::phone::sms_consent` |
| P68 | A US sender with a campaign `ACTIVE` but `TCR_PENDING`, through REST and the MCP tool | `403 a2p_not_registered` on both | S | `core::phone::a2p::readiness`, `it::phone::a2p_gate_all_paths` |
| P69 | Marketing to France at 23:00 Paris time; transactional at the same time | `409 quiet_hours` with `next_allowed_at`; sent | S | `core::phone::catalogue::quiet_hours`, `it::phone::quiet_hours` |
| P70 | Segment caps reached; `TenantQuota` unreachable | `429 daily_cap_reached` with the cap; `503 unavailable`, never a pass | S | `it::phone::sms_caps_fail_closed` |
| P71 | A destination country not allowed; a premium-rate or short-code destination | `403 policy_denied` (`destination_country`); `403 destination_blocked` | S | `it::phone::sms_destinations` |
| P72 | `message.sent` from a different sender than requested | `sender_used` recorded, `sender_substituted: true`, metric | S | `it::phone::sender_substitution` |
| P73 | A spend cap reached; the cap raised; the same key retried | `402 spend_cap_reached` with nothing stored; then accepted | S | `it::phone::spend_cap` |
| P74 | Monthly charges that exceed the cap | Charged; `phone_spend.warning` at 100%; numbers kept | S | `it::phone::recurring_over_cap` |
| P75 | A cost-bearing event delivered twice | One charge | S | `it::phone::charge_unique_source` |
| P76 | A quote read by a tenant key and by a partner key | GBP only; GBP and `quote.provider` | S | `it::phone::quote_visibility` |
| P77 | The invoicing job runs twice for a month | One Stripe invoice item per tenant and category (Stripe idempotency key) | S | `it::phone::invoicing_idempotent` |
| P78 | A partner statement | Only that partner's tenants | S | `it::phone::partner_statement_scope` |
| P79 | Provider balance under 7 days of spend; under 2 days | Page alert; `paused_purchases` automatically, SMS and calls continue | S | `it::phone::balance_low` |
| P80 | The provider's daily spend limit hit (40333) | `failed` (`provider_spend_limit`); `phone_provider_limit_hit` | S | `it::phone::provider_limit` |
| P81 | Brand `VETTED_VERIFIED`; an unknown status | Approved; unchanged with `phone_status_unknown` | S | `core::phone::telnyx::a2p_status_table` |
| P82 | Brand creation repeated after a crash | The existing brand adopted by tag or EIN; no second fee; campaigns by `referenceId` | S | `it::phone::a2p_no_double_fee` |
| P83 | `10dlc.campaign.update` reaches `/hooks/telnyx`; `MNO_PROVISIONED` | Mapped; numbers assigned and ready | S | `it::phone::a2p_events` |
| P84 | A simulated ID written for a live tenant | Refused by the table `CHECK` | S | `it::phone::a2p_no_sim_ids` |
| P85 | A `vapi` route | Vapi credential once per connector; number adopted or created; PATCH then GET must match, else `voice_configuration`; no Telnyx key sent to Vapi | S | `it::phone::vapi_route`, `live::phone::vapi_route` |
| P86 | Vapi answers `401` | Route `failed`; tenant notified | S | `it::phone::vapi_revoked` |
| P87 | An outbound call under `approval` | An `outbound_call` request; on approval placed once with the disclosure first; an unknown outcome `uncertain` and reconciled from Vapi's call list | S | `it::phone::outbound_call_flow` |
| P88 | A call to an emergency or premium number; policy `off`; a non-`vapi` route | `403 destination_blocked`; `403 policy_denied` (`calls_off`); `409 voice_route_unsupported` | S | `it::phone::outbound_call_refusals` |
| P89 | A `forward` route to the tenant's own number, or to a disallowed country | Refused at request creation (`400 invalid_request`) | S | `it::phone::forward_rules` |
| P90 | Call events and call records; an outbound call the service did not place | `calls` rows and `call.*` events; usage counts as caps count; `phone_unplaced_outbound_call` and a notification | S | `it::phone::call_records`, `it::phone::unplaced_call` |
| P91 | A US number without an emergency address | Its routes place no calls; an approved `enable_emergency` request registers one | S | `it::phone::us_emergency` |
| P92 | A port-in the provider splits; an exception; a cancel within 48 hours of FOC | One `pti_` and number row per draft; `action_needed`; `409 port_in_not_cancelable` | S | `it::phone::port_in` |
| P93 | A port-out request | `port_out_pending`, page alert, urgent emails, first in "Needs a person", `respond_by` 24 hours, escalations at 50%, 75%, 90% | S | `it::phone::port_out_notice` |
| P94 | Nobody decides a port-out | Never rejected automatically; the provider's authorisation completes it; `ported_out`, route removed, PIN deleted, history kept | S | `it::phone::port_out_auto_authorised` |
| P95 | A port-out rejected (code 1001 without text; with text) | `400 invalid_request`; back to `active` and the PIN rotated | S | `it::phone::port_out_reject` |
| P96 | Nightly: a tagged provider number with no row; a live row missing at the provider; configuration drift | `phone_orphan_number`, never deleted; `suspended` (`missing_at_provider`); repaired with read-back | S | `it::phone::nightly_reconcile` |
| P97 | A webhook delivery that failed at the provider | Found in `webhook_deliveries`; the resource re-read; counted | S | `it::phone::missed_webhooks` |
| P98 | Tenant erasure with numbers, open requests, documents and campaigns | `phone_release` cancels, releases, deactivates, deletes resources and connectors, shreds documents except retained ones, listed in the receipt | S | `it::phone::tenant_erasure` |
| P99 | KYC after the last number; a platform export | Deleted after `PM_PHONE_KYC_RETENTION_DAYS`; export behind a single-use link with an audit row | S | `it::phone::kyc_retention_and_export` |
| P100 | Adopting a number already in the directory or stamped for another tenant; a legacy-group number | Refused; adopted with `legacy_group` and notifications at 60, 30 and 7 days | S | `it::phone::adopt` |
| P101 | The ops switch at `paused_purchases` and `paused`; `PM_PHONE` turned off and on | Refusals as § 30; no number released; on re-enable the reconciler runs first | S | `it::phone::switch_and_rollback` |
| P102 | `put_managed` fails at activation | The number stays `configuring`; retried; `phone_pin_missing` after an hour | S | `it::phone::pin_vault_unavailable` |
| P103 | Every phone route with keys of another tenant, another identity and another partner | The same `404` body as a missing resource | S | `it::phone::scope_matrix` |
| P104 | A provisional agent sign-up | `phone:*` not effective: `403 permission_denied` (`signup_unverified`) | S | `it::phone::provisional_signup` |
| P105 | An opt-in recorded by an identity key; an opt-out by an identity key with `phone:send` | `403 permission_denied`; recorded | S | `it::phone::consent_permissions` |
| P106 | A dry-run send | `would_send`, segments and price; no hold and no row | S | `it::phone::sms_dry_run` |
| P107 | Pylota moves from its own Telnyx integration | Its handler takes Pylota Identity's signed events keyed on `evt_`; it sends no STOP confirmation; its old Telnyx key and `TELNYX_*` environment are retired | I | Pylota's `phone-identity-cutover` suite |

## 2. Preamble

In the first paragraph, update the counts to add 107 rows in one section (106 owned by the service, `S`; P107
owned by the integrator, `I`), add P107 to the list of integrator-owned rows, and add section P to the list of
sections that follow W. The preamble explains that section letters avoid U (selling points), M (milestones),
P (priorities) and S (spikes). Section P was allocated to this design by the coordinator's brief; if the
coordinator keeps P, add this sentence after the one about U: "Section P holds phone-number rows: its IDs are
always written with the section letter and a number from 1 to 107 (P1–P107), so they cannot be mistaken for
the priorities P0, P1 and P2, which are never used as row IDs; in prose a row is always called 'row P1' and a
priority 'priority P1'." Otherwise rename the rows to the letter chosen, here and in
`design/phone-numbers.md`, in one change.
