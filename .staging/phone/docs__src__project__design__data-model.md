Where this goes: `docs/src/project/design/data-model.md`. (1) Rows added to the ID prefix table in "## Conventions"; (2) a new migration block at the end of "## 1. D1 control plane" (after the last `CREATE` statement of the section, before "### Notes"); (3) amendments to `platform_state` and `service_accounts`; (4) a new block at the end of "## 2. `IdentityMailbox` Durable Object (SQLite)" (before "### Mailbox notes"); (5) one row in "## 4. R2 objects"; (6) a paragraph in "### Notes". Source: [Phone numbers](phone-numbers.md).

## 1. Prefix table rows

| `pnm_` | phone number | `phr_` | phone request |
| `pdr_` | phone onboarding draft | `cpp_` | phone compliance profile |
| `cpd_` | phone compliance document | `a2p_` | US messaging registration |
| `pvc_` | voice connector | `vrt_` | voice route |
| `pto_` | port-out | `pti_` | port-in record |
| `chg_` | phone charge | `pev_` | phone provider event (local ID) |
| `sms_` | SMS message (mailbox) | `smc_` | SMS conversation (mailbox) |
| `cal_` | call (mailbox) | | |

## 2. D1 tables (end of "## 1. D1 control plane")

```sql
-- migrations/d1/00NN_phone.sql (M38; M39 and M40 add the tables marked with their milestone in later files)

-- Phone numbers § 8: one row per tenant with its provider resources, created before the first billable action.
CREATE TABLE phone_tenants (
  tenant_id                 TEXT PRIMARY KEY REFERENCES tenants(id),
  provider                  TEXT NOT NULL DEFAULT 'telnyx' CHECK (provider IN ('telnyx')),
  provider_account_id       TEXT NOT NULL,                 -- PM_TELNYX_ACCOUNT_ID at creation
  messaging_profile_id      TEXT,
  connection_id             TEXT,                          -- the tenant's credential connection
  outbound_voice_profile_id TEXT,
  billing_group_id          TEXT,
  sip_user                  TEXT,
  sip_sealed                BLOB,                          -- pm1 envelope of the SIP password (sealed.rs)
  resources_status          TEXT NOT NULL DEFAULT 'pending'
                            CHECK (resources_status IN ('pending','ready','failed')),
  sync_due_at               INTEGER,                       -- a policy change waits to be applied (§ 8)
  last_checked_at           INTEGER,
  created_at                INTEGER NOT NULL,
  updated_at                INTEGER NOT NULL
);

-- Phone numbers § 7.1: the directory and history. A row is never deleted while its identity exists.
CREATE TABLE phone_numbers (
  id                     TEXT PRIMARY KEY,                 -- pnm_
  tenant_id              TEXT NOT NULL REFERENCES tenants(id),
  identity_id            TEXT NOT NULL REFERENCES identities(id),
  e164                   TEXT NOT NULL,
  country                TEXT NOT NULL,                    -- ISO 3166-1 alpha-2
  number_type            TEXT NOT NULL CHECK (number_type IN ('mobile','local','national','toll_free')),
  catalogue_capabilities_json TEXT NOT NULL,               -- the row's capabilities at purchase (§ 3.3)
  provider               TEXT NOT NULL DEFAULT 'telnyx',
  provider_account_id    TEXT NOT NULL,
  provider_number_id     TEXT,
  provider_order_id      TEXT,
  provider_sub_order_id  TEXT,
  request_id             TEXT,                             -- the purchase, port-in or adoption that made it
  status                 TEXT NOT NULL CHECK (status IN ('ordering','uncertain','under_review','requirements_pending',
                           'configuring','active','suspended','port_out_pending','porting_in','releasing',
                           'released','ported_out','failed')),
  status_reason          TEXT,                             -- e.g. missing_at_provider, legacy_unpapered, moved
  carrier_status         TEXT NOT NULL DEFAULT 'pending' CHECK (carrier_status IN ('pending','active','failed')),
  sms_status             TEXT NOT NULL CHECK (sms_status IN ('not_applicable','pending','configuring','ready','failed')),
  voice_status           TEXT NOT NULL CHECK (voice_status IN ('not_applicable','pending','configuring','ready','failed')),
  a2p_status             TEXT NOT NULL DEFAULT 'not_applicable'
                         CHECK (a2p_status IN ('not_applicable','not_registered','pending','ready','failed')),
  emergency_status       TEXT NOT NULL DEFAULT 'not_applicable'
                         CHECK (emergency_status IN ('not_applicable','none','pending','registered')),
  primary_sms            INTEGER NOT NULL DEFAULT 0 CHECK (primary_sms IN (0,1)),
  primary_voice          INTEGER NOT NULL DEFAULT 0 CHECK (primary_voice IN (0,1)),
  provisioning_code      TEXT,                             -- § 6.4 code set
  provisioning_retryable INTEGER CHECK (provisioning_retryable IN (0,1)),
  requirements_status    TEXT,
  deadline_at            INTEGER,
  action_needed_json     TEXT,                             -- {kind, message (untrusted), deadline_at}
  review_started_at      INTEGER,
  compliance_status      TEXT NOT NULL DEFAULT 'own' CHECK (compliance_status IN ('own','legacy_group')),
  legacy_deadline        INTEGER,                          -- § 1.1; NULL until the sole-trader decision
  expires_at             INTEGER,                          -- § 6.8
  port_pin               TEXT NOT NULL DEFAULT 'missing' CHECK (port_pin IN ('set','missing')),
  adopted_at             INTEGER,
  active_from            INTEGER,
  active_until           INTEGER,
  last_event_at          INTEGER,
  next_check_at          INTEGER,
  checks                 INTEGER NOT NULL DEFAULT 0,
  client_reference       TEXT,
  created_at             INTEGER NOT NULL,
  updated_at             INTEGER NOT NULL
);
CREATE UNIQUE INDEX phone_numbers_live ON phone_numbers(e164)
  WHERE status IN ('ordering','uncertain','under_review','requirements_pending','configuring','active','suspended',
                   'port_out_pending','porting_in','releasing');
CREATE INDEX phone_numbers_identity ON phone_numbers(identity_id, status);
CREATE INDEX phone_numbers_tenant ON phone_numbers(tenant_id, status);
CREATE INDEX phone_numbers_due ON phone_numbers(next_check_at) WHERE next_check_at IS NOT NULL;
CREATE INDEX phone_numbers_expiring ON phone_numbers(expires_at) WHERE expires_at IS NOT NULL;

-- Phone numbers § 7.5: released numbers withheld from other tenants for 90 days.
CREATE TABLE phone_number_releases (
  e164_hash   TEXT PRIMARY KEY,                            -- hex HMAC(PM_HASH_KEY, e164)
  tenant_id   TEXT NOT NULL,
  released_at INTEGER NOT NULL,
  purge_at    INTEGER NOT NULL                             -- released_at + 90 days
);

-- Phone numbers § 5: the object a person (or the standing budget) approves.
CREATE TABLE phone_requests (
  id                   TEXT PRIMARY KEY,                   -- phr_
  tenant_id            TEXT NOT NULL REFERENCES tenants(id),
  identity_id          TEXT NOT NULL REFERENCES identities(id),
  kind                 TEXT NOT NULL CHECK (kind IN ('purchase_number','port_in','release_number','submit_compliance',
                         'register_us_a2p','verify_toll_free','enable_emergency','set_voice_route','outbound_call')),
  payload_json         TEXT NOT NULL,                      -- normalised (core::phone::requests::normalise)
  quote_json           TEXT,                               -- GBP; with follow_on[] for purchases
  digest               TEXT NOT NULL,                      -- 64 hex
  status               TEXT NOT NULL CHECK (status IN ('pending_approval','approved','executing','in_progress',
                         'action_needed','uncertain','completed','failed','rejected','canceled')),
  reason               TEXT,
  blocked_by_json      TEXT NOT NULL DEFAULT '[]',         -- ["compliance","platform_prerequisite"] (§ 5.8)
  unblocked_at         INTEGER,
  approved_by          TEXT CHECK (approved_by IN ('person','key','standing_budget','purchase')),
  budget_version       INTEGER,                            -- with approved_by = 'standing_budget'
  parent_request_id    TEXT,                               -- a scheduled release's purchase (§ 6.8)
  release_at           INTEGER,                            -- purchase with expires_at
  requested_by_key_id  TEXT,
  requested_by_user_id TEXT,
  decided_by_key_id    TEXT,
  decided_by_user_id   TEXT,
  on_behalf_of         TEXT,
  note                 TEXT,
  decided_at           INTEGER,
  attempt              INTEGER NOT NULL DEFAULT 0,
  candidates_tried     INTEGER NOT NULL DEFAULT 0,         -- taken numbers replaced (§ 6.7), at most 5
  customer_reference   TEXT,                               -- pi:{id}:{attempt}
  spend_intent_at      INTEGER,
  exec_token           TEXT,
  exec_until           INTEGER,
  next_check_at        INTEGER,
  checks               INTEGER NOT NULL DEFAULT 0,
  result_json          TEXT,
  secrets_sealed       BLOB,                               -- port-in losing-carrier details; cleared at the end
  client_reference     TEXT,
  expires_at           INTEGER,                            -- pending only (§ 5.2, § 5.8)
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);
CREATE INDEX phone_requests_tenant ON phone_requests(tenant_id, status, created_at);
CREATE INDEX phone_requests_identity ON phone_requests(identity_id, status, created_at);
CREATE INDEX phone_requests_due ON phone_requests(next_check_at) WHERE next_check_at IS NOT NULL;
CREATE INDEX phone_requests_release ON phone_requests(release_at) WHERE release_at IS NOT NULL;
CREATE UNIQUE INDEX phone_requests_reference ON phone_requests(customer_reference) WHERE customer_reference IS NOT NULL;

-- Phone numbers § 4.7: registration text an agent drafted for a person to submit. No personal data by schema.
CREATE TABLE phone_onboarding_drafts (
  id                 TEXT PRIMARY KEY,                     -- pdr_
  tenant_id          TEXT NOT NULL REFERENCES tenants(id),
  country            TEXT NOT NULL,
  fields_json        TEXT NOT NULL,                        -- validated against the draft schema (untrusted text)
  drafted_by_key_id  TEXT,
  drafted_by_user_id TEXT,
  purge_at           INTEGER NOT NULL,                     -- updated_at + 30 days
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL,
  UNIQUE (tenant_id, country)
);

-- Phone numbers § 4.1: the legal entity numbers are registered under.
CREATE TABLE phone_compliance_profiles (
  id                    TEXT PRIMARY KEY,                  -- cpp_
  tenant_id             TEXT NOT NULL UNIQUE REFERENCES tenants(id),
  legal_name            TEXT NOT NULL,
  trading_name          TEXT,
  entity_type           TEXT NOT NULL CHECK (entity_type IN ('limited_company','llp','partnership','public_company',
                          'non_profit','sole_trader')),       -- sole_trader accepted only per § 1.1
  registration_country  TEXT NOT NULL,
  company_number        TEXT,
  company_registry_json TEXT,                              -- {country, region, name}
  tax_ids_sealed        BLOB,
  website               TEXT,
  use_case              TEXT,
  representative_json   TEXT,                              -- {name, role}
  representative_sealed BLOB,                              -- email, phone, birth data where a row needs them
  addresses_json        TEXT NOT NULL DEFAULT '[]',        -- [{id, kind, lines, locality, region, postal_code, country, verified}]
  created_at            INTEGER NOT NULL,
  updated_at            INTEGER NOT NULL
);

-- Phone numbers § 4.2: write-only KYC documents.
CREATE TABLE phone_compliance_documents (
  id                   TEXT PRIMARY KEY,                   -- cpd_
  tenant_id            TEXT NOT NULL REFERENCES tenants(id),
  profile_id           TEXT NOT NULL REFERENCES phone_compliance_profiles(id),
  kind                 TEXT NOT NULL CHECK (kind IN ('company_registration_certificate','proof_of_address',
                         'representative_id','registration_form','power_of_attorney','letter_of_authorisation',
                         'invoice','other')),
  filename             TEXT NOT NULL,
  content_type         TEXT NOT NULL CHECK (content_type IN ('application/pdf','image/png','image/jpeg')),
  size                 INTEGER NOT NULL,
  sha256               TEXT NOT NULL,
  valid_until          INTEGER,
  status               TEXT NOT NULL CHECK (status IN ('pending_upload','ready','deleted')),
  key_sealed           BLOB,                               -- the document's data key; deleting it shreds the document
  provider_document_id TEXT,
  hold_reason          TEXT CHECK (hold_reason IN ('kyc_retention')),
  retain_until         INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);
CREATE INDEX phone_compliance_documents_tenant ON phone_compliance_documents(tenant_id, status);

-- Phone numbers § 4.3 and § 4.4: one row per tenant, country, number type and action.
CREATE TABLE phone_requirement_groups (
  id                   TEXT PRIMARY KEY,
  tenant_id            TEXT NOT NULL REFERENCES tenants(id),
  profile_id           TEXT NOT NULL REFERENCES phone_compliance_profiles(id),
  country              TEXT NOT NULL,
  number_type          TEXT NOT NULL,
  action               TEXT NOT NULL CHECK (action IN ('ordering','porting')),
  provider_group_id    TEXT,
  status               TEXT NOT NULL CHECK (status IN ('draft','under_review','approved','rejected','expired')),
  provider_status      TEXT,
  comments             TEXT,                               -- provider comments, untrusted
  requirements_version TEXT,
  requirements_json    TEXT,                               -- the requirement set submitted against (cached 24 h)
  expires_at           INTEGER,
  last_checked_at      INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL,
  UNIQUE (tenant_id, country, number_type, action)
);

-- Phone numbers § 13 (M39): brands, campaigns and toll-free verifications.
CREATE TABLE phone_registrations (
  id                   TEXT PRIMARY KEY,                   -- a2p_
  tenant_id            TEXT NOT NULL REFERENCES tenants(id),
  kind                 TEXT NOT NULL CHECK (kind IN ('brand','campaign','toll_free')),
  brand_entity         TEXT CHECK (brand_entity IN ('standard','sole_proprietor')),
  brand_country        TEXT,
  provider_brand_id    TEXT CHECK (provider_brand_id IS NULL OR provider_brand_id NOT LIKE 'sim-%'),
  provider_campaign_id TEXT,
  reference_id         TEXT UNIQUE,                        -- campaigns: pi:{request_id}
  status               TEXT NOT NULL CHECK (status IN ('draft','submitted','pending','ready','failed','deactivated')),
  provider_status_json TEXT,                               -- identityStatus, campaignStatus, verification status
  otp_status           TEXT CHECK (otp_status IN ('not_required','sent','verified','expired')),
  otp_resends_today    INTEGER NOT NULL DEFAULT 0,
  failure_reasons_json TEXT,
  throughput_json      TEXT,
  request_id           TEXT,
  last_event_at        INTEGER,
  next_check_at        INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);
CREATE INDEX phone_registrations_tenant ON phone_registrations(tenant_id, kind, status);

-- Phone numbers § 14.2 (M40): a tenant's Vapi connection.
CREATE TABLE phone_voice_connectors (
  id                 TEXT PRIMARY KEY,                     -- pvc_
  tenant_id          TEXT NOT NULL REFERENCES tenants(id),
  kind               TEXT NOT NULL CHECK (kind IN ('vapi')),
  region             TEXT NOT NULL CHECK (region IN ('us','eu')),
  api_key_sealed     BLOB NOT NULL,
  vapi_credential_id TEXT,
  status             TEXT NOT NULL CHECK (status IN ('active','failed','deleted')),
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);

-- Phone numbers § 14.1 (M40): one route per voice-capable number.
CREATE TABLE phone_voice_routes (
  id                   TEXT PRIMARY KEY,                   -- vrt_
  tenant_id            TEXT NOT NULL REFERENCES tenants(id),
  number_id            TEXT NOT NULL UNIQUE REFERENCES phone_numbers(id),
  kind                 TEXT NOT NULL CHECK (kind IN ('vapi','sip','forward','none')),
  config_json          TEXT NOT NULL,
  connector_id         TEXT REFERENCES phone_voice_connectors(id),
  server_secret_sealed BLOB,
  vapi_phone_number_id TEXT,
  declares_recording   INTEGER NOT NULL DEFAULT 0 CHECK (declares_recording IN (0,1)),
  status               TEXT NOT NULL CHECK (status IN ('pending','ready','failed')),
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);

-- Phone numbers § 15 (M41): one record per provider porting order of a port-in request.
CREATE TABLE phone_port_ins (
  id                     TEXT PRIMARY KEY,                 -- pti_
  tenant_id              TEXT NOT NULL REFERENCES tenants(id),
  request_id             TEXT NOT NULL REFERENCES phone_requests(id),
  provider_porting_order_id TEXT,
  status                 TEXT NOT NULL,
  foc_at                 INTEGER,
  comments               TEXT,                             -- provider comments, untrusted
  created_at             INTEGER NOT NULL,
  updated_at             INTEGER NOT NULL
);

-- Phone numbers § 16 (M41): port-outs and their decisions.
CREATE TABLE phone_port_outs (
  id                   TEXT PRIMARY KEY,                   -- pto_
  tenant_id            TEXT NOT NULL REFERENCES tenants(id),
  number_id            TEXT NOT NULL REFERENCES phone_numbers(id),
  provider_port_out_id TEXT NOT NULL UNIQUE,
  status               TEXT NOT NULL CHECK (status IN ('pending','authorized','rejected','rejected_pending','ported','canceled')),
  respond_by           INTEGER NOT NULL,
  decision             TEXT CHECK (decision IN ('authorized','rejected','provider_authorized')),
  reason_code          TEXT,
  reason_text          TEXT,
  decided_by_key_id    TEXT,
  decided_by_user_id   TEXT,
  on_behalf_of         TEXT,
  decided_at           INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);

-- Phone numbers § 10.5 (M39): the consent ledger, per tenant brand and counterparty hash.
CREATE TABLE phone_consent (
  tenant_id         TEXT NOT NULL REFERENCES tenants(id),
  counterparty_hash TEXT NOT NULL,                         -- hex HMAC(PM_HASH_KEY, e164)
  hint              TEXT NOT NULL,                         -- masked, e.g. +44 •••• ••0123
  state             TEXT NOT NULL CHECK (state IN ('opted_out','opted_in')),
  purpose           TEXT CHECK (purpose IN ('marketing','all')),
  source            TEXT NOT NULL CHECK (source IN ('keyword','api','console','provider_sync')),
  evidence          TEXT,                                  -- untrusted, ≤ 1,000 chars
  sms_id            TEXT,
  updated_by_key_id TEXT,
  updated_at        INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, counterparty_hash)
);

-- Phone numbers § 18.2: one row per charge, unique by source.
CREATE TABLE phone_charges (
  id                     TEXT PRIMARY KEY,                 -- chg_
  tenant_id              TEXT NOT NULL REFERENCES tenants(id),
  identity_id            TEXT,
  number_id              TEXT,
  kind                   TEXT NOT NULL CHECK (kind IN ('number_setup','number_monthly','sms_out','sms_in','mms_in',
                           'voice_in','voice_out','voice_forward','a2p_fee','emergency_monthly','port_fee')),
  pence                  INTEGER NOT NULL,
  provider_cost_usd      TEXT,                             -- decimal string
  card_version           INTEGER NOT NULL,
  month                  TEXT NOT NULL,                    -- YYYY-MM, UTC
  source                 TEXT NOT NULL UNIQUE,             -- provider event ID or monthly:{number_id}:{YYYY-MM}
  invoiced_at            INTEGER,
  stripe_invoice_item_id TEXT,
  created_at             INTEGER NOT NULL
);
CREATE INDEX phone_charges_tenant_month ON phone_charges(tenant_id, month);

-- Phone numbers § 9.5: provider events, deduplicated by the provider's event ID.
CREATE TABLE phone_provider_events (
  provider_event_id TEXT PRIMARY KEY,
  id                TEXT NOT NULL UNIQUE,                  -- pev_
  provider          TEXT NOT NULL DEFAULT 'telnyx',
  event_type        TEXT NOT NULL,
  occurred_at       INTEGER NOT NULL,
  resource_id       TEXT,
  tenant_id         TEXT,
  status            TEXT NOT NULL CHECK (status IN ('pending','processed','ignored_mismatch','ignored_unrouted',
                      'pending_target')),
  r2_key            TEXT NOT NULL,
  created_at        INTEGER NOT NULL,
  processed_at      INTEGER
);
CREATE INDEX phone_provider_events_pending ON phone_provider_events(status, created_at)
  WHERE status IN ('pending','pending_target');

-- Phone numbers § 12.3 (M39): provider message ID → owner, written at dispatch.
CREATE TABLE phone_message_index (
  provider_message_id TEXT PRIMARY KEY,
  tenant_id           TEXT NOT NULL,
  identity_id         TEXT NOT NULL,
  sms_id              TEXT NOT NULL,
  created_at          INTEGER NOT NULL
);
```

## 3. Amendments

- `platform_state.key`: add `'phone_account'` to the `CHECK` list. Its `value_json` is
  `{"level": "paid" | "verified" | "unknown", "probed_at": <ms>, "us_enabled": bool}`, written by `idvy setup
  phone` and `idvy doctor` through `POST /v1/platform/phone/probe` (platform:ops) and read by the catalogue
  eligibility and the setup status ([Phone numbers § 2.1](phone-numbers.md#21-one-account-and-provider_account_id-everywhere)).
- `service_accounts`: add `phone_number_id TEXT` (the receiving number a code is expected on) and
  `sms_senders_json TEXT` (a JSON array of up to 10 senders), both nullable, written by
  `POST …/accounts` and read by the SMS verification gate ([Phone numbers § 11.2](phone-numbers.md#112-the-gate)).
- The tenant policy's `phone.*` fields live in the existing policy JSON, including `phone.auto_approve`; every
  change is a policy version, as other guard fields are.

## 4. Mailbox tables (end of "## 2. `IdentityMailbox` Durable Object (SQLite)")

```sql
-- crates/worker/src/mailbox/schema/00NN_sms.sql (M39)
CREATE TABLE sms_conversations (
  id                TEXT PRIMARY KEY,                      -- smc_
  number_id         TEXT NOT NULL,                         -- pnm_ of this identity
  counterparty      TEXT NOT NULL,                         -- E.164, short code or alphanumeric sender
  counterparty_hash TEXT NOT NULL,
  last_message_at   INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  UNIQUE (number_id, counterparty)
);

CREATE TABLE sms_messages (
  id                  TEXT PRIMARY KEY,                    -- sms_
  conversation_id     TEXT NOT NULL REFERENCES sms_conversations(id),
  number_id           TEXT NOT NULL,
  direction           TEXT NOT NULL CHECK (direction IN ('inbound','outbound')),
  status              TEXT NOT NULL CHECK (status IN ('received','quarantined','screening','hidden','throttled',
                        'queued','sent_to_provider','sent','delivered','failed','delivery_unconfirmed',
                        'uncertain','reconciled')),
  quarantine_reason   TEXT,
  kind                TEXT CHECK (kind IN ('transactional','marketing','verification')),
  from_party          TEXT NOT NULL,
  to_party            TEXT NOT NULL,
  text                TEXT,                                -- NFC, ≤ 10,000 chars; codes stay here, redacted on read
  text_digest         TEXT,
  truncated           INTEGER NOT NULL DEFAULT 0,
  segments            INTEGER,
  encoding            TEXT,
  flags_json          TEXT NOT NULL DEFAULT '["sender_unverified"]',
  screening_json      TEXT,
  provider_message_id TEXT UNIQUE,
  sender_used         TEXT,
  sender_substituted  INTEGER NOT NULL DEFAULT 0,
  reason              TEXT,
  client_reference    TEXT,
  received_at         INTEGER,
  sent_at             INTEGER,
  last_event_at       INTEGER,
  created_at          INTEGER NOT NULL
);
CREATE INDEX sms_messages_conversation ON sms_messages(conversation_id, created_at);
CREATE INDEX sms_messages_status ON sms_messages(status, created_at);

CREATE TABLE sms_media (
  sms_id       TEXT NOT NULL REFERENCES sms_messages(id),
  n            INTEGER NOT NULL,
  content_type TEXT,
  size         INTEGER,
  status       TEXT NOT NULL CHECK (status IN ('pending','stored','unavailable','risky')),
  r2_key       TEXT,
  PRIMARY KEY (sms_id, n)
);

CREATE TABLE sms_verifications (
  sms_id            TEXT PRIMARY KEY REFERENCES sms_messages(id),
  number_id         TEXT NOT NULL,
  sender            TEXT NOT NULL,
  account_id        TEXT,                                  -- the ledger entry that matched
  vault_item_id     TEXT,                                  -- a vault login item's phone channel claimed it
  kind              TEXT NOT NULL CHECK (kind IN ('code')),
  value             TEXT NOT NULL,                         -- never logged, never in an event
  solicited         INTEGER NOT NULL CHECK (solicited IN (0,1)),
  consumed_at       INTEGER,
  vault_consumed_at INTEGER,
  vault_use_id      TEXT,
  received_at       INTEGER NOT NULL,
  expires_at        INTEGER NOT NULL                       -- received_at + 24 h
);

CREATE TABLE sms_waits (
  id          TEXT PRIMARY KEY,
  number_id   TEXT NOT NULL,
  from_party  TEXT,
  account_id  TEXT,
  expires_at  INTEGER NOT NULL
);

-- crates/worker/src/mailbox/schema/00NN_calls.sql (M40)
CREATE TABLE calls (
  id                 TEXT PRIMARY KEY,                     -- cal_
  number_id          TEXT NOT NULL,
  direction          TEXT NOT NULL CHECK (direction IN ('inbound','outbound')),
  counterparty       TEXT NOT NULL,
  route_kind         TEXT,
  provider_call_id   TEXT UNIQUE,
  vapi_call_id       TEXT,
  status             TEXT NOT NULL CHECK (status IN ('placing','ringing','in_progress','ended','uncertain')),
  outcome            TEXT CHECK (outcome IN ('answered','no_answer','busy','failed','rejected')),
  disclosure_applied INTEGER NOT NULL DEFAULT 0,
  started_at         INTEGER,
  answered_at        INTEGER,
  ended_at           INTEGER,
  duration_seconds   INTEGER,
  cost_pence         INTEGER,
  source             TEXT NOT NULL DEFAULT 'webhook' CHECK (source IN ('webhook','cdr')),
  created_at         INTEGER NOT NULL
);
CREATE INDEX calls_number ON calls(number_id, created_at);
```

The mailbox `idempotency` table gains `sms_id TEXT` (nullable), the SMS a send key produced.

## 5. R2 objects ("## 4. R2 objects")

| Key | Holds | Written by | Deleted by |
|---|---|---|---|
| `phone-staging/{yyyy}/{mm}/{dd}/{event_id}.json` | A raw provider event | `POST /hooks/telnyx` | The consumer after processing; a lifecycle rule after 15 days |
| `t/{ten}/phone/docs/{cpd}` | A KYC document, AES-256-GCM under its sealed data key | The upload link | Document delete, retention, tenant erasure |
| `t/{ten}/i/{idn}/sms/{sms}/m/{n}` | An inbound MMS item | The `pm-phone` consumer | Retention and erasure, with its SMS |

## 6. In "### Notes", add

- **Phone numbers.** `phone_numbers` is both the routing directory (one live row per E.164, enforced by
  `phone_numbers_live`) and the history of ownership periods. Requests, numbers, registrations and port-outs
  move only under the execution lease of [Phone numbers § 5.5](phone-numbers.md#55-execution-leases-and-attempts);
  `customer_reference` is unique so a recovered order can be attributed to exactly one attempt. The standing
  budget's month count and the phone spend holds live in `TenantQuota` (`phone_budget:t:*`, `phone_spend:*`),
  not in D1.
