# Data model

Binding for implementation. Migrations live in `migrations/d1/` (D1) and in
`crates/worker/src/mailbox/schema/` (Durable Object SQLite, applied on wake). This page is the source
of truth for both.

## Conventions

- **IDs** are a type prefix, an underscore, and a [ULID](https://github.com/ulid/spec) (26 characters,
  Crockford base32, upper case). For example: `msg_01J9Z3K8V4QW7X2M5N6P8R0T1Y`. ULIDs come from the
  platform clock and RNG (never `SystemTime` in wasm). They are monotonic within a millisecond per isolate.

  | Prefix | Entity | Prefix | Entity |
  |---|---|---|---|
  | `ten_` | tenant | `whk_` | webhook endpoint |
  | `idn_` | identity | `dlv_` | webhook delivery |
  | `adr_` | address | `evt_` | event |
  | `dom_` | domain | `key_` | API key |
  | `thr_` | thread | `era_` | erasure request |
  | `msg_` | message | `exp_` | export |
  | `att_` | attachment | `job_` | internal job |
  | `aud_` | audit entry | `req_` | request ID |
  | `usr_` | console user | `inv_` | invitation |
  | `dlq_` | dead-letter item | `hld_` | quota hold |
  | `prb_` | alignment probe | `ptn_` | partner |
  | `sac_` | service-ledger entry | `drf_` | draft |
  | `evc_` | durable event consumer | `wsc_` | WebSocket connection |
  | `sfb_` | screening feedback | `upl_` | upload (mailbox) |
  | `asu_` | agent sign-up | `pnm_` | phone number |
  | `phr_` | phone request | `pdr_` | phone onboarding draft |
  | `cpp_` | phone compliance profile | `cpd_` | phone compliance document |
  | `a2p_` | US messaging registration | `pvc_` | voice connector |
  | `vrt_` | voice route | `pto_` | port-out |
  | `pti_` | port-in record | `chg_` | phone charge |
  | `pev_` | phone provider event (local ID) | `sms_` | SMS message (mailbox) |
  | `smc_` | SMS conversation (mailbox) | `cal_` | call (mailbox) |
  | `vit_` | vault item | `vgr_` | vault grant |
  | `vus_` | vault use | `vcn_` | vault consumer |
  | `vad_` | vault audit row | `vtk_` | vault ticket (never stored; its SHA-256 only) |

- **Times** are stored as Unix milliseconds (`INTEGER`) and exposed in the API as RFC 3339 UTC strings.
- **Email addresses** are stored lower-cased, with the domain as an IDNA A-label (punycode). Local parts
  keep dots (no provider-specific folding, [A1](../edge-cases.md)).
- **JSON columns** end in `_json` and hold values validated by `crates/api-types` before writing.
- **Enumerations** are `TEXT` with a `CHECK` constraint, so an invalid state fails at write time.
- D1 runs with `PRAGMA foreign_keys = ON`. Durable Object SQLite enables it in each migration.
- **Every column has a writer and a reader** named in a design. The only exception is bookkeeping time:
  `created_at`, `updated_at` and `schema_migrations.applied_at` are written on every insert (and
  `updated_at` on every update), as is the mailbox's `meta.created_at`, and kept for support and incident work even where no design reads them.
- **Every Durable Object** keeps `meta.schema_version`, written by its first migration and checked on
  wake ([Design conventions §4, rule 6](index.md#4-durable-object-transactions)).

## 1. D1 control plane

```sql
-- migrations/d1/0001_init.sql
PRAGMA foreign_keys = ON;

-- An integrator whose partner keys create tenants and act only on those tenants (Security § 4.6).
-- Never deleted: DELETE /v1/partners/{id} sets status 'deleted' and scrubs name (Privacy § 6.10), so
-- tenants.partner_id always points at a row.
CREATE TABLE partners (
  id                   TEXT PRIMARY KEY,                   -- ptn_
  name                 TEXT NOT NULL,                      -- the only data a partner holds; '' once deleted
  status               TEXT NOT NULL DEFAULT 'active'
                       CHECK (status IN ('active','suspended','deleted')),
  default_billing_mode TEXT NOT NULL DEFAULT 'metered'     -- copied to billing_accounts.mode of each tenant
                       CHECK (default_billing_mode IN ('exempt','metered')),  -- a partner key creates
  max_tenants          INTEGER NOT NULL DEFAULT 25         -- tenants not erased, at most (platform-set)
                       CHECK (max_tenants >= 0),
  ramp_exempt          INTEGER NOT NULL DEFAULT 0          -- 1: its tenants skip the new-workspace send ramp
                       CHECK (ramp_exempt IN (0,1)),       -- (platform-set; Cloud sign-up § 10.1)
  deleted_at           INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);

CREATE TABLE tenants (
  id               TEXT PRIMARY KEY,                       -- ten_
  slug             TEXT NOT NULL UNIQUE,                   -- ^[a-z0-9][a-z0-9-]{1,31}$
  name             TEXT NOT NULL,
  partner_id       TEXT REFERENCES partners(id),           -- the partner whose key created the tenant; NULL
                                                           -- otherwise. Written at insert and never updated,
                                                           -- also after erasure and partner deletion (partners
                                                           -- are soft-deleted), so provenance is kept
  mode             TEXT NOT NULL CHECK (mode IN ('live','test')),
  status           TEXT NOT NULL DEFAULT 'active'
                   CHECK (status IN ('active','suspended','erasing','erased')),
  suspended_at     INTEGER,
  suspended_by     TEXT CHECK (suspended_by IN ('platform','partner')),  -- who suspended it; NULL while not
                                                           -- suspended. A partner key cannot lift 'platform'
  address_suffix   TEXT NOT NULL,                          -- '' (the default tenant only) or '.' followed by
                                                           -- 2–32 of [a-z0-9-], starting with a letter or digit
                                                           -- (openapi AddressSuffix); defaults to '.' || slug,
                                                           -- and a workspace may choose another (Cloud sign-up § 7)
  suffix_fold      TEXT NOT NULL,                          -- fold (Identities › Confusable detection) of the suffix
                                                           -- without its dot, rewritten with it; '' for the default
                                                           -- tenant. Unique, so no tenant gets a look-alike of
                                                           -- another's suffix (D12)
  timezone         TEXT NOT NULL DEFAULT 'UTC',            -- IANA name
  sending_paused_at    INTEGER,                            -- tenant send pause (Outbound › Tenant and domain
  sending_pause_reason TEXT                                -- auto-pause, G12): set by the delivery consumer, cleared
                       CHECK (sending_pause_reason IN ('abuse_threshold','abuse_burst')),  -- only by a platform key
  policy_json      TEXT NOT NULL,                          -- TenantPolicy (see configuration.md)
  policy_ceilings_json TEXT NOT NULL DEFAULT '{}',         -- lower-only policy fields a platform key set, with
                                                           -- the value it set: a ceiling for partner keys and
                                                           -- workspace writers (Configuration › Who may change a field)
  partner_ceilings_json TEXT NOT NULL DEFAULT '{}',        -- lower-only policy fields the tenant's partner key set,
                                                           -- with the value: a ceiling for workspace writers only
                                                           -- (Workspace policy § 2)
  policy_version   INTEGER NOT NULL DEFAULT 0,             -- compare-and-set counter of policy writes (Workspace
                                                           -- policy § 4); +1 with every write of policy_json
  quota_do_id      TEXT NOT NULL,                          -- TenantQuota Durable Object id; minted with the row,
                                                           -- then QuotaRequest::Init { tenant_id }
  notify_do_id     TEXT NOT NULL,                          -- Notifier Durable Object id; minted with the row,
                                                           -- then NotifierRequest::Init { tenant_id } (Notifications § 8);
                                                           -- '' on rows written before M26 builds the Notifier;
                                                           -- from M26 the every-minute cron mints a Notifier and
                                                           -- sends Init for each row still at '', as it mints
                                                           -- mailbox_do_id
  events_do_id     TEXT NOT NULL DEFAULT '',               -- TenantEvents Durable Object id; minted with the row;
                                                           -- '' on rows written before M30, minted by the
                                                           -- every-minute cron; Init on first use (Event feed § 2.1)
  require_two_factor      INTEGER NOT NULL DEFAULT 0       -- members need two-step verification (console)
                          CHECK (require_two_factor IN (0,1)),
  onboarding_dismissed_at INTEGER,                         -- first-run checklist dismissed: written by the dismiss
                                                           -- action, read by the Overview render (Cloud sign-up § 8)
  ramp_lifted_at   INTEGER,                                -- new-workspace send ramp ended (Cloud sign-up § 10.1):
                                                           -- set by the daily evaluation (crons/signup_ramp.rs) or
                                                           -- by the billing webhook on a paid plan; read at
                                                           -- outbound policy step 18
  provisional_until INTEGER,                               -- set while the tenant is a provisional agent sign-up:
                                                           -- when it expires. Written by the create batch, cleared
                                                           -- by verification; read by key authentication (the
                                                           -- permission overlay) and core::policy::effective
                                                           -- (Agent self-sign-up § 15)
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL
);
CREATE UNIQUE INDEX tenants_suffix ON tenants(address_suffix);   -- '' included: one default tenant only
CREATE UNIQUE INDEX tenants_suffix_fold ON tenants(suffix_fold) WHERE suffix_fold <> '';
CREATE INDEX tenants_partner ON tenants(partner_id) WHERE partner_id IS NOT NULL;

CREATE TABLE domains (
  id                    TEXT PRIMARY KEY,                  -- dom_
  tenant_id             TEXT REFERENCES tenants(id),       -- NULL only for the platform domain
  name                  TEXT NOT NULL,                     -- A-label, lower case; unique among rows not
                                                           -- 'removed' (domains_name_live): a re-added
                                                           -- name gets a new row, the removed row stays
  kind                  TEXT NOT NULL CHECK (kind IN ('platform','zone','delegated','external')),
  method                TEXT NOT NULL                      -- connection method; fixes kind, inbound, transport
                        CHECK (method IN ('platform','cloudflare_zone','nameservers','dns_records',
                                          'send_only','smtp_relay','delegated_subdomain')),
  zone_id               TEXT,                              -- Cloudflare zone (platform, zone, delegated)
  is_apex               INTEGER NOT NULL CHECK (is_apex IN (0,1)),
  routing_mode          TEXT NOT NULL CHECK (routing_mode IN ('catch_all','literal','forward')),
  inbound               TEXT NOT NULL CHECK (inbound IN ('routing','ses','forward','none')),
  transport             TEXT NOT NULL CHECK (transport IN ('cloudflare','ses','smtp')),
  reply_token           TEXT NOT NULL CHECK (reply_token IN ('subaddress','none')),
  receiving             INTEGER NOT NULL CHECK (receiving IN (0,1)),
  sending               INTEGER NOT NULL CHECK (sending IN (0,1)),
  state                 TEXT NOT NULL
                        CHECK (state IN ('pending','verifying','healthy','degraded','failing',
                                         'suspended','removing','removed')),
  state_reason          TEXT,                              -- machine code, e.g. dkim_missing
  state_changed_at      INTEGER NOT NULL,
  ownership_token       TEXT,                              -- value for _pylota-identity TXT challenge
  ownership_verified_at INTEGER,                           -- NULL until first verified: such a domain counts
                                                           -- toward the unverified cap, expires after 14 days
                                                           -- and can be evicted (identity-domains.md)
  expected_ns_json      TEXT,                              -- zone name servers, written with the row
  rdap_fingerprint      TEXT,                              -- hash of registrar + registrant handle + created;
                                                           -- written by the first RDAP query after verification
  provider_objects_json TEXT NOT NULL DEFAULT '{}',        -- provider objects onboarding created or adopted,
                                                           -- by provider ID (identity-domains.md › Provider
                                                           -- objects); removal acts on these only
  event_subscription_id TEXT,                              -- Email Sending → pm-delivery-events; NULL on a
                                                           -- cloudflare-transport domain = delivery_events
                                                           -- "manual" (S9 fallback, identity-domains.md)
  ses_identity          TEXT,                              -- SES email identity name, if inbound or transport = ses,
                                                           -- or the J5 failover identity of a cloudflare_zone,
                                                           -- nameservers or delegated_subdomain domain
  ses_region            TEXT,                              -- set whenever ses_identity is
  mail_from_domain      TEXT,                              -- pm-bounce.{domain}, the custom MAIL FROM of dns_records
                                                           -- and send_only; NULL for a J5 failover identity
  smtp_sealed           BLOB,                              -- smtp_relay: pm1 envelope of {host, port, username,
                                                           -- password, probe_from}
  smtp_pending_sealed   BLOB,                              -- values from PATCH waiting for a passing probe
                                                           -- (pm1 envelope, aad column smtp_pending_sealed)
  probe_last_at         INTEGER,                           -- transport = smtp: last alignment probe; written by
  probe_last_json       TEXT,                              -- DomainMonitor, read by the health check and GET domain
                                                           -- {result, dkim_d, dmarc, from_unchanged, at,
                                                           --  failures_in_row, pending}
  records_json          TEXT NOT NULL DEFAULT '[]',        -- records to publish, with their last observed state;
                                                           -- each carries name (FQDN), host (relative to the
                                                           -- registrable domain), purpose and required
  monitor_do_id         TEXT NOT NULL,                     -- DomainMonitor Durable Object id; '' until the
                                                           -- every-minute cron mints it (rows written by the CLI)
  sending_paused_at     INTEGER,                           -- domain send pause at provider-level abuse rates (G12);
                                                           -- set by the delivery consumer, cleared only by a
                                                           -- platform key; a paused domain never falls back
  created_at            INTEGER NOT NULL,
  updated_at            INTEGER NOT NULL
);
CREATE INDEX domains_tenant ON domains(tenant_id, state);
CREATE UNIQUE INDEX domains_name_live ON domains(name) WHERE state <> 'removed';
CREATE INDEX domains_zone ON domains(zone_id, inbound) WHERE zone_id IS NOT NULL AND state <> 'removed';

-- A domain add in progress: written before the first provider call, deleted by the batch that inserts
-- the domain row, or by the hourly cleanup after undoing its objects (identity-domains.md › Adding a
-- domain, Cleanup after a failed add).
CREATE TABLE domain_onboarding (
  name         TEXT PRIMARY KEY,                           -- A-label, lower case; one add per name at a time
  domain_id    TEXT NOT NULL,                              -- the ID the domain row will get
  tenant_id    TEXT NOT NULL REFERENCES tenants(id),
  method       TEXT NOT NULL,
  zone_id      TEXT,                                       -- the zone, once known
  objects_json TEXT NOT NULL DEFAULT '{}',                 -- provider objects created so far (same shape as
                                                           -- domains.provider_objects_json)
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE INDEX domain_onboarding_stale ON domain_onboarding(updated_at);

-- Cloudflare zones this deployment created for a tenant (nameservers, delegated_subdomain), so no other
-- tenant's key can use them through cloudflare_zone, and only these zones are ever deleted
-- (Identities and domains › Zone permission, Creating a zone).
CREATE TABLE zone_claims (
  zone_name  TEXT PRIMARY KEY,                             -- A-label apex of the zone
  zone_id    TEXT UNIQUE,                                  -- Cloudflare zone ID; NULL while pending
  tenant_id  TEXT NOT NULL REFERENCES tenants(id),         -- the tenant it was created for
  domain_id  TEXT NOT NULL,                                -- the domain whose onboarding created it
  state      TEXT NOT NULL CHECK (state IN ('pending','active')),  -- pending: written before POST /zones
  created_at INTEGER NOT NULL,
  CHECK ((state = 'active') = (zone_id IS NOT NULL))
);

CREATE TABLE identities (
  id                 TEXT PRIMARY KEY,                     -- idn_
  tenant_id          TEXT NOT NULL REFERENCES tenants(id),
  username           TEXT NOT NULL,                        -- ^[a-z0-9][a-z0-9._-]{0,23}$
  display_name       TEXT NOT NULL,                        -- ≤ 78 chars, no CR/LF
  purpose            TEXT,                                 -- free tag, e.g. bookings
  client_id          TEXT,                                 -- integrator's idempotent create key
  client_fingerprint TEXT,                                 -- sha256 of the canonical create body
  owner_name         TEXT,                                 -- accountable human (FR-IDN-2)
  owner_email        TEXT,
  signature_text     TEXT,
  signature_html     TEXT,                                 -- sanitised on write
  status             TEXT NOT NULL DEFAULT 'active'
                     CHECK (status IN ('active','paused','deleting','deleted')),
  pause_reason       TEXT CHECK (pause_reason IN ('manual','abuse_threshold','tenant_suspended')),
  send_policy_json   TEXT NOT NULL DEFAULT '{}',
  message_hours      INTEGER CHECK (message_hours IS NULL OR message_hours BETWEEN 1 AND 8760),  -- retention.message_hours (Parity additions § 7)
  metadata_json      TEXT NOT NULL DEFAULT '{}',           -- ≤ 16 string keys, ≤ 512 bytes each
  mailbox_do_id      TEXT NOT NULL,                        -- IdentityMailbox Durable Object id; '' until the
                                                           -- every-minute cron mints it (the system identity)
  is_system          INTEGER NOT NULL DEFAULT 0            -- 1 only for the system identity (PM_SYSTEM_FROM):
                     CHECK (is_system IN (0,1)),           -- never listed to tenants, username not validated
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);
CREATE UNIQUE INDEX identities_system   ON identities(is_system) WHERE is_system = 1;
CREATE UNIQUE INDEX identities_client   ON identities(tenant_id, client_id) WHERE client_id IS NOT NULL;
CREATE UNIQUE INDEX identities_username ON identities(tenant_id, username) WHERE status <> 'deleted';
CREATE INDEX identities_tenant ON identities(tenant_id, status);

-- The address directory: every inbound message is routed with one lookup on addresses.address.
CREATE TABLE addresses (
  id              TEXT PRIMARY KEY,                        -- adr_
  address         TEXT NOT NULL,                           -- local@domain, lower case, A-label
  local_part      TEXT NOT NULL,
  domain_id       TEXT NOT NULL REFERENCES domains(id),
  tenant_id       TEXT NOT NULL REFERENCES tenants(id),
  identity_id     TEXT NOT NULL REFERENCES identities(id),
  role            TEXT NOT NULL CHECK (role IN ('primary','alias')),
  status          TEXT NOT NULL CHECK (status IN ('pending','active','retiring','retired')),
  routing_rule_id TEXT,                                    -- Cloudflare literal rule (routing_mode = literal)
  retire_at       INTEGER,                                 -- when status = retiring
  retired_at      INTEGER,
  ses_bounce_rule TEXT,                                    -- pm-retired-{n} holding this retired address (inbound = ses)
  forwarding      TEXT                                     -- NULL unless the domain's inbound = forward
                  CHECK (forwarding IN ('unverified','ok','failed')),
  forwarding_checked_at INTEGER,                           -- last forwarding-check token result (ordinary mail never
                                                           -- changes forwarding, Inbound › Multiple identities)
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
);
CREATE UNIQUE INDEX addresses_address     ON addresses(address);
CREATE UNIQUE INDEX addresses_one_primary ON addresses(identity_id) WHERE role = 'primary';
CREATE INDEX addresses_identity ON addresses(identity_id, status);
CREATE INDEX addresses_retiring ON addresses(status, retire_at) WHERE status = 'retiring';

-- Deleted or erased addresses can never be reassigned (A5). Stored as a keyed hash, so an erased
-- address is not kept in clear.
CREATE TABLE address_tombstones (
  address_hash TEXT PRIMARY KEY,                           -- hex HMAC-SHA256(PM_HASH_KEY, address)
  identity_id  TEXT,                                       -- the only identity allowed to reclaim it
  reason       TEXT NOT NULL CHECK (reason IN ('deleted','erased'))
);

CREATE TABLE api_keys (
  id                TEXT PRIMARY KEY,                      -- key_
  lookup            TEXT NOT NULL UNIQUE,                  -- 12 chars, embedded in the secret
  hash              TEXT NOT NULL,                         -- hex HMAC-SHA256(PM_KEY_PEPPER, secret)
  prev_hash         TEXT,                                  -- previous secret during rotation overlap
  prev_expires_at   INTEGER,
  name              TEXT NOT NULL,
  level             TEXT NOT NULL CHECK (level IN ('platform','partner','tenant','identity')),
  partner_id        TEXT REFERENCES partners(id),          -- partner keys only: the partner they act for
  tenant_id         TEXT REFERENCES tenants(id),
  identity_id       TEXT REFERENCES identities(id),
  mode              TEXT NOT NULL CHECK (mode IN ('live','test')),
  permissions_json  TEXT NOT NULL,                         -- array of permission strings
  mcp_tools_json    TEXT CHECK (mcp_tools_json IS NULL OR json_type(mcp_tools_json) = 'array'),
                                                           -- NULL = every tool the permissions and level
                                                           -- allow; else a JSON array of 0–64 tool names
                                                           -- (SDKs, skills and agent tooling § 5.3)
  created_by_key_id TEXT,
  created_by_user_id TEXT REFERENCES users(id),            -- usr_: the person behind the key (console mint, or
                                                           -- copied from the parent key); NULL otherwise
  created_by_role   TEXT CHECK (created_by_role IN ('owner','admin')),  -- that person's role when it was set;
                                                           -- 'owner' lets a tenant key hold tenants:erase
  created_by_level  TEXT NOT NULL CHECK (created_by_level IN ('platform','partner','tenant','identity',
                                                             'console','setup')),  -- who minted it: the
                                                           -- caller's level, a console session, or idvy setup;
                                                           -- platform/partner = provider-managed (W48)
  expires_at        INTEGER,
  revoked_at        INTEGER,
  last_used_at      INTEGER,                               -- updated at most once per minute
  created_at        INTEGER NOT NULL,
  CHECK ((level = 'platform' AND partner_id IS NULL AND tenant_id IS NULL AND identity_id IS NULL)
      OR (level = 'partner'  AND partner_id IS NOT NULL AND tenant_id IS NULL AND identity_id IS NULL)
      OR (level = 'tenant'   AND partner_id IS NULL AND tenant_id IS NOT NULL AND identity_id IS NULL)
      OR (level = 'identity' AND partner_id IS NULL AND tenant_id IS NOT NULL AND identity_id IS NOT NULL))
);
CREATE INDEX api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX api_keys_partner ON api_keys(partner_id) WHERE partner_id IS NOT NULL;
CREATE INDEX api_keys_creator ON api_keys(tenant_id, created_by_user_id) WHERE created_by_user_id IS NOT NULL;

CREATE TABLE webhook_endpoints (
  id                     TEXT PRIMARY KEY,                 -- whk_
  tenant_id              TEXT REFERENCES tenants(id),      -- NULL = platform or partner endpoint
  partner_id             TEXT REFERENCES partners(id),     -- partner endpoint (scope "partner"); NULL otherwise
  url                    TEXT NOT NULL,                    -- https only, validated (SSRF rules)
  description            TEXT,
  event_types_json       TEXT NOT NULL,                    -- ["*"] or explicit list
  identity_ids_json      TEXT,                             -- optional filter
  enabled                INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
  disabled_reason        TEXT                              -- manual | failing (a 410 is 'failing'; the event says gone)
                         CHECK (disabled_reason IN ('manual','failing','url_changed','creator_removed')),
                                                           -- | url_changed: a PATCH changed the URL; a passing test
                                                           -- re-enables (J29) | creator_removed: the person behind
                                                           -- it left or lost webhooks:manage (W47)
  created_by_key_id      TEXT,                             -- the key that created it, or last re-enabled it
  created_by_user_id     TEXT REFERENCES users(id),        -- usr_: the person behind that key or console session
                                                           -- (api_keys.created_by_user_id); NULL otherwise
  secret_enc             TEXT NOT NULL,                    -- AES-256-GCM(PM_MASTER_KEY), base64
  prev_secret_enc        TEXT,
  prev_secret_expires_at INTEGER,
  headers_sealed         TEXT,                             -- pm1 envelope of the custom delivery headers (JSON map),
                                                           -- or NULL (Parity additions § 8)
  header_names_json      TEXT NOT NULL DEFAULT '[]',       -- the names only, for reads
  headers_version        INTEGER NOT NULL DEFAULT 0,       -- compare-and-swap for PATCH …/headers
  consecutive_failures   INTEGER NOT NULL DEFAULT 0,
  created_at             INTEGER NOT NULL,
  updated_at             INTEGER NOT NULL,
  CHECK (tenant_id IS NULL OR partner_id IS NULL)          -- scope: tenant, partner, or platform (both NULL)
);
CREATE INDEX webhook_endpoints_tenant ON webhook_endpoints(tenant_id, enabled);
CREATE INDEX webhook_endpoints_partner ON webhook_endpoints(partner_id, enabled) WHERE partner_id IS NOT NULL;

CREATE TABLE webhook_deliveries (
  id              TEXT PRIMARY KEY,                        -- dlv_
  endpoint_id     TEXT NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
  tenant_id       TEXT,
  event_id        TEXT NOT NULL,
  event_type      TEXT NOT NULL,
  attempt         INTEGER NOT NULL,
  status          TEXT NOT NULL CHECK (status IN ('succeeded','failed','dead')),
  http_status     INTEGER,
  error           TEXT,                                    -- machine code: timeout, tls, dns, status_5xx, ...
  duration_ms     INTEGER,
  next_attempt_at INTEGER,
  created_at      INTEGER NOT NULL
);
CREATE UNIQUE INDEX webhook_deliveries_attempt ON webhook_deliveries(endpoint_id, event_id, attempt);
CREATE INDEX webhook_deliveries_endpoint ON webhook_deliveries(endpoint_id, created_at DESC);

-- Deliveries parked while their partner is suspended (J13), instead of re-queuing every 15 minutes.
-- Written by the Deliver consumer; the every-minute cron re-queues the rows of partners that are active
-- again, and records rows past the replay window as dead (event_unavailable). Webhooks › Delivering.
CREATE TABLE webhook_held (
  endpoint_id   TEXT NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
  event_id      TEXT NOT NULL,
  partner_id    TEXT NOT NULL,                             -- the suspended partner (the endpoint's, or its tenant's)
  job_json      TEXT NOT NULL,                             -- the Deliver message, re-sent unchanged
  occurred_at   INTEGER NOT NULL,                          -- the event's, for the replay window
  held_at       INTEGER NOT NULL,
  PRIMARY KEY (endpoint_id, event_id)
);
CREATE INDEX webhook_held_partner ON webhook_held(partner_id, held_at);

-- Transport circuit breakers (Outbound › Transport circuit breaker, J28). One row per transport scope:
-- 'cloudflare' (the account), 'ses:{region}', 'smtp:{domain_id}'.
CREATE TABLE transport_breakers (
  scope               TEXT PRIMARY KEY,
  consecutive_unknown INTEGER NOT NULL DEFAULT 0,          -- Unknown outcomes in a row; reset by a definitive one
  state               TEXT NOT NULL DEFAULT 'closed' CHECK (state IN ('closed','open','half_open')),
  open_until          INTEGER,                             -- while open: no claims before this time
  open_count          INTEGER NOT NULL DEFAULT 0,          -- consecutive openings; doubles the open period
  probe_message_id    TEXT,                                -- half_open: the one message allowed through
  updated_at          INTEGER NOT NULL
);

-- Where each event's payload lives (owner object), for replay. Payloads stay in the owner.
CREATE TABLE event_index (
  id           TEXT PRIMARY KEY,                           -- evt_
  tenant_id    TEXT,
  identity_id  TEXT,
  type         TEXT NOT NULL,
  owner_kind   TEXT NOT NULL CHECK (owner_kind IN ('mailbox','domain','job','platform')),
  owner_id     TEXT NOT NULL,                              -- Durable Object id, or 'platform'
  partner_id   TEXT,                                       -- the event tenant's partner (tenants.partner_id, which
                                                           -- never changes), or for webhook.disabled the disabled
                                                           -- endpoint's partner; NULL otherwise. Read by replay
                                                           -- to a partner endpoint (Webhooks § Replay)
  payload_json TEXT,                                       -- only for owner_kind = 'platform'
  occurred_at  INTEGER NOT NULL,
  fanned_out_at INTEGER                                    -- platform events only: set by the Fanout consumer,
                                                           -- read by the outbox sweep (Webhooks § Platform events)
);
CREATE INDEX event_index_tenant_time ON event_index(tenant_id, occurred_at);
CREATE INDEX event_index_partner_time ON event_index(partner_id, occurred_at) WHERE partner_id IS NOT NULL;

CREATE TABLE suppressions (
  tenant_id         TEXT NOT NULL REFERENCES tenants(id),
  address_hash      TEXT NOT NULL,                         -- HMAC(PM_HASH_KEY, address)
  address_hint      TEXT NOT NULL,                         -- masked, e.g. j***@example.com
  reason            TEXT NOT NULL
                    CHECK (reason IN ('hard_bounce','complaint','unsubscribe','manual','provider')),
  source_message_id TEXT,
  note              TEXT,
  created_at        INTEGER NOT NULL,
  expires_at        INTEGER,                               -- NULL = permanent
  PRIMARY KEY (tenant_id, address_hash)
);

-- Allow and block lists, per tenant or per identity (Parity additions § 3.5). '' stands for the tenant scope,
-- because SQLite treats NULL values in a composite primary key as distinct.
CREATE TABLE sender_lists (
  tenant_id   TEXT NOT NULL REFERENCES tenants(id),
  identity_id TEXT NOT NULL DEFAULT '',                    -- '' = tenant scope; else an idn_ of this tenant
  direction   TEXT NOT NULL CHECK (direction IN ('receive','send')),
  kind        TEXT NOT NULL CHECK (kind IN ('allow','block')),
  entry       TEXT NOT NULL,                               -- user@example.com or @example.com
  note        TEXT,
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, identity_id, direction, kind, entry)
);
CREATE INDEX sender_lists_identity ON sender_lists(identity_id, direction) WHERE identity_id <> '';

CREATE TABLE jobs (
  id                TEXT PRIMARY KEY,                      -- job_
  tenant_id         TEXT,
  kind              TEXT NOT NULL
                    CHECK (kind IN ('erasure','retention','export','reembed','reparse','reindex','domain_remove',
                                    'backup','restore_reconcile','event_log_rebuild')),
  status            TEXT NOT NULL CHECK (status IN ('queued','running','completed','failed','canceled')),
  runner_do_id      TEXT NOT NULL,                         -- JobRunner Durable Object id
  params_json       TEXT NOT NULL,                         -- JobRequest::Start params; a counterparty erasure may
                                                           -- carry internal-only identity_ids (Privacy § 6.4)
  result_json       TEXT,
  created_by_key_id TEXT,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  completed_at      INTEGER
);
CREATE INDEX jobs_tenant ON jobs(tenant_id, kind, created_at DESC);

CREATE TABLE erasure_requests (
  id                   TEXT PRIMARY KEY,                   -- era_
  tenant_id            TEXT NOT NULL,
  job_id               TEXT NOT NULL REFERENCES jobs(id),
  scope                TEXT NOT NULL
                       CHECK (scope IN ('message','thread','counterparty','identity','tenant')),
  identity_id          TEXT,
  target_id            TEXT,                               -- msg_/thr_ for message and thread scope
  counterparty_hash    TEXT,                               -- HMAC of the counterparty address
  reason               TEXT NOT NULL,
  status               TEXT NOT NULL
                       CHECK (status IN ('queued','running','completed','completed_with_holds','failed',
                                         'canceled')),
  receipt_json         TEXT,
  created_by_key_id    TEXT,
  resumes_id           TEXT REFERENCES erasure_requests(id), -- the failed request this one resumes, or the
                                                           -- completed_with_holds one it continues (Privacy § 6.1)
  deadline_at          INTEGER NOT NULL,                   -- created_at + 24 h; copied from the failed request
                                                           -- when resuming one. Read by the job's retry limit and
                                                           -- the erasure_overdue alert (Privacy § 4)
  created_at           INTEGER NOT NULL,
  completed_at         INTEGER
);
CREATE INDEX erasure_requests_tenant ON erasure_requests(tenant_id, scope, created_at DESC);
CREATE UNIQUE INDEX erasure_requests_resumes ON erasure_requests(resumes_id)
  WHERE resumes_id IS NOT NULL;                            -- one resumption or continuation per request (Privacy § 6.1)

CREATE TABLE exports (
  id            TEXT PRIMARY KEY,                          -- exp_
  tenant_id     TEXT NOT NULL,
  job_id        TEXT NOT NULL REFERENCES jobs(id),
  scope         TEXT NOT NULL CHECK (scope IN ('counterparty','identity')),
  identity_id   TEXT,                                      -- identity scope; returned in the export object
  status        TEXT NOT NULL CHECK (status IN ('queued','running','completed','failed','canceled','expired')),
  r2_key        TEXT,
  size          INTEGER,
  expires_at    INTEGER,                                   -- the ZIP is kept 7 days; each download link lives
                                                           -- 1 hour (Privacy § 9.4); DELETE /v1/exports/{id}
                                                           -- deletes the ZIP at once and sets 'expired'
  created_at    INTEGER NOT NULL
);

CREATE TABLE link_uses (                                   -- export links work once (Privacy § 9.4)
  nonce       TEXT PRIMARY KEY,                            -- the link payload's nonce
  expires_at  INTEGER NOT NULL                             -- the link's expiry; the global retention job
);                                                         -- deletes rows past it

-- Durable Object calls that must follow a D1 change (Design conventions § 9): written in the same D1 batch as
-- the change, deleted once the call succeeds; the every-minute cron re-sends the rest.
CREATE TABLE rpc_intents (
  id          TEXT PRIMARY KEY,                            -- deterministic: '{request_id}:{target_id}:{op}', or
                                                           -- '{request_id}:{identity_id}:{event_type}' per identity
  class       TEXT NOT NULL CHECK (class IN ('quota','mailbox')),
  object_id   TEXT NOT NULL,                               -- tenants.quota_do_id or identities.mailbox_do_id
  tenant_id   TEXT NOT NULL REFERENCES tenants(id),
  identity_id TEXT,                                        -- mailbox intents
  op          TEXT NOT NULL CHECK (op IN ('init','emit_event','set_retention')),  -- set_retention: the identity's
                                                           -- retention.message_hours (Parity additions § 7.2)
  body_json   TEXT NOT NULL,                               -- the request body (InitMailbox, EmitEvent, SetRetention, …): IDs and
                                                           -- a thin event payload, never message content
  occurred_at INTEGER NOT NULL,                            -- the D1 change time: the event time, and the time part
                                                           -- of the derived event ID
  attempts    INTEGER NOT NULL DEFAULT 0,
  next_at     INTEGER NOT NULL,                            -- occurred_at + 60 s, then backoff (Design § 9)
  created_at  INTEGER NOT NULL
);
CREATE INDEX rpc_intents_due ON rpc_intents(next_at);
CREATE INDEX rpc_intents_tenant ON rpc_intents(tenant_id);

-- Idempotency for non-mail POSTs (mail sends are idempotent inside the mailbox).
CREATE TABLE idempotency_records (
  scope           TEXT NOT NULL,                           -- tenant_id; the partner_id for a partner key's POST
                                                           -- that names no tenant (POST /v1/tenants,
                                                           -- POST /v1/webhooks); or 'platform'
  key_id          TEXT NOT NULL,                           -- the calling API key: another key in the same scope
                                                           -- never receives this record's replay
  tenant_id       TEXT,                                    -- the tenant the stored response belongs to (the scope
                                                           -- tenant, or the tenant a POST /v1/tenants created);
                                                           -- tenant erasure deletes by it (Privacy § 6.6)
  identity_id     TEXT,                                    -- the identity the stored response belongs to (an
                                                           -- identity create, its addresses and keys, and the
                                                           -- mail routes of the content rule below); identity
                                                           -- erasure deletes by it (Privacy § 6.5)
  idem_key        TEXT NOT NULL,                           -- ≤ 255 printable ASCII
  method          TEXT NOT NULL,
  path            TEXT NOT NULL,
  fingerprint     TEXT NOT NULL,                           -- sha256(method, path, canonical JSON body)
  status          TEXT NOT NULL CHECK (status IN ('in_progress','completed')),
  response_status INTEGER,
  response_body   TEXT,                                    -- never a one-time secret (below)
  created_at      INTEGER NOT NULL,
  expires_at      INTEGER NOT NULL,                        -- created_at + 30 days
  PRIMARY KEY (scope, key_id, idem_key)
);
CREATE INDEX idempotency_records_tenant ON idempotency_records(tenant_id) WHERE tenant_id IS NOT NULL;
CREATE INDEX idempotency_records_identity ON idempotency_records(identity_id) WHERE identity_id IS NOT NULL;
-- Every non-mail POST with an Idempotency-Key (metered ones as in Billing › What the Worker meters):
--   1. SELECT by (scope, key_id, idem_key). completed: same method, path and fingerprint → replay
--      response_status and response_body with Idempotent-Replayed: true; different → 409 idempotency_conflict.
--      in_progress and created_at within 60 s → 409 request_in_progress; older (the first request died)
--      → take it over: UPDATE … SET created_at = now WHERE status = 'in_progress' AND created_at = ?old.
--   2. Otherwise INSERT (status 'in_progress'); a primary-key conflict → 409 request_in_progress.
--   3. Run the action, then UPDATE status = 'completed', response_status, response_body (≤ 64 KB; a
--      larger body stores the resource ID and the replay re-reads it), and tenant_id. A response that
--      carries a one-time secret (POST /v1/keys, POST /v1/keys/{id}/rotate, POST /v1/webhooks,
--      POST /v1/tenants/{t}/webhooks, POST /v1/webhooks/{id}/rotate-secret) is stored with `secret`
--      removed and "secret_replayed": false added, so a replay returns that body and the secret is kept
--      nowhere (FR-KEY-2).
--      Content rule: no record ever holds mail content. A response that carries a Message or a thread
--      summary (POST …/release, …/cancel, …/resolve, …/threads/{t}/hold) is stored as a reference only,
--      response_body = {"$ref":{"kind":"message"|"thread","identity_id":…,"id":…}} with identity_id set,
--      and a replay re-reads the resource through the same handler with the calling key's visibility (a
--      resource erased since replays its 404). Endpoints that return mail and change nothing (POST …/search
--      at both scopes) are x-idempotency: none and are never recorded.
--   A 4xx or 5xx before the action changed anything deletes the row, so the same key can be retried.

-- Agent signing keys (Agent signing keys § 2 and § 8). Generated, sealed and used only inside the Worker.
CREATE TABLE identity_keys (
  id           TEXT PRIMARY KEY,                           -- RFC 7638 thumbprint of the public JWK, base64url (the kid)
  identity_id  TEXT NOT NULL REFERENCES identities(id),
  tenant_id    TEXT NOT NULL,
  alg          TEXT NOT NULL CHECK (alg = 'EdDSA'),
  public_jwk   TEXT NOT NULL,                              -- {kty: OKP, crv: Ed25519, x, kid, alg, use}
  private_enc  BLOB NOT NULL,                              -- pm1 envelope of the 32-byte Ed25519 seed
  status       TEXT NOT NULL CHECK (status IN ('active','retiring','retired')),
  created_at   INTEGER NOT NULL,
  verify_until INTEGER,                                    -- set when retiring: now + PM_IDENTITY_KEY_OVERLAP_DAYS
  retired_at   INTEGER
);
CREATE UNIQUE INDEX identity_keys_one_active ON identity_keys (identity_id) WHERE status = 'active';

-- The service sign-up ledger (Service sign-up ledger § 2): one row per third-party account an agent asked
-- to create. Written by the …/accounts routes, the console's accounts page, the global retention job
-- (expiry, purge) and erasure; read by those routes, the inbound consumer (step 12, approved rows) and
-- the wait handler.
CREATE TABLE service_accounts (
  id                  TEXT PRIMARY KEY,                     -- sac_
  tenant_id           TEXT NOT NULL REFERENCES tenants(id),
  identity_id         TEXT NOT NULL REFERENCES identities(id),
  service_domain      TEXT NOT NULL,                        -- organisational domain, A-label, lower case
  sender_domains_json TEXT NOT NULL,                        -- JSON array of organisational domains, 1–6,
                                                            -- always including service_domain
  account_identifier  TEXT NOT NULL,                        -- username or account email at the service, ≤ 254
  address             TEXT NOT NULL,                        -- the identity's address the service mails
  purpose             TEXT NOT NULL,                        -- ≤ 500 chars, written by the agent (untrusted)
  status              TEXT NOT NULL CHECK (status IN ('pending_approval','approved','rejected','closed')),
  rejected_reason     TEXT CHECK (rejected_reason IN ('operator','expired')),
  note                TEXT,                                 -- decision or closing note, ≤ 500 chars
  requested_by_key_id TEXT,
  decided_by_key_id   TEXT,
  decided_by_user_id  TEXT,
  decided_at          INTEGER,
  closed_by_key_id    TEXT,
  closed_by_user_id   TEXT,
  closed_at           INTEGER,
  expires_at          INTEGER,                              -- pending only: created_at + 7 days
  phone_number_id     TEXT,                                 -- pnm_: the number a code is expected on (Phone
                                                            -- numbers § 11.2)
  sms_senders_json    TEXT,                                 -- JSON array of up to 10 SMS senders; read by the SMS gate
  login_domains_json  TEXT NOT NULL DEFAULT '[]',          -- 0–5 organisational domains where the account
                                                            -- signs in (Vault § 3.3); read by the vault's origin rule
  created_at          INTEGER NOT NULL,
  updated_at          INTEGER NOT NULL
);
CREATE UNIQUE INDEX service_accounts_live ON service_accounts(identity_id, service_domain, account_identifier)
  WHERE status IN ('pending_approval','approved');
CREATE INDEX service_accounts_identity ON service_accounts(identity_id, status, created_at);
CREATE INDEX service_accounts_tenant ON service_accounts(tenant_id, status, created_at);

-- Drafts, approvals and scheduled send § 9.2: one row per draft that is pending_approval, scheduled or failed;
-- written only by the draft's mailbox (§ 9.3); read by GET /v1/tenants/{tenant_id}/drafts, the console's
-- Overview and drafts page, the Notifier's needs-a-person count and the drafts_due watchdog. It holds no mail
-- content.
CREATE TABLE draft_index (
  draft_id            TEXT PRIMARY KEY,                    -- drf_
  tenant_id           TEXT NOT NULL REFERENCES tenants(id),
  identity_id         TEXT NOT NULL REFERENCES identities(id),
  status              TEXT NOT NULL CHECK (status IN ('pending_approval','scheduled','failed')),
  version             INTEGER NOT NULL,                    -- the mailbox's drafts.index_version
  revision            INTEGER NOT NULL,
  draft_kind          TEXT NOT NULL CHECK (draft_kind IN ('new','reply','reply_all','forward')),
  approval_reason     TEXT CHECK (approval_reason IN ('requested','new_recipients','external','always')),
  recipient_count     INTEGER NOT NULL,
  attachment_count    INTEGER NOT NULL,
  send_at             INTEGER,
  next_attempt_at     INTEGER,                             -- a scheduled draft waiting after a transient condition
  status_at           INTEGER NOT NULL,
  failure_reason      TEXT,
  requested_by_key_id TEXT,
  source_held         INTEGER NOT NULL DEFAULT 0,          -- 1 while the source is quarantined or in screening
  updated_at          INTEGER NOT NULL
);
CREATE INDEX draft_index_tenant ON draft_index(tenant_id, status, status_at, draft_id);
CREATE INDEX draft_index_identity ON draft_index(identity_id, status);
CREATE INDEX draft_index_due ON draft_index(COALESCE(next_attempt_at, send_at)) WHERE status = 'scheduled';

-- Thumbprints of deleted identity keys, never published again (O7). Written by identity- and tenant-scope
-- erasure in the step that deletes identity_keys; read by key generation, which draws a new seed when the
-- thumbprint of a new key is found here. Key IDs are not personal data and are never deleted.
CREATE TABLE key_tombstones (
  kid        TEXT PRIMARY KEY,                             -- RFC 7638 thumbprint, base64url
  deleted_at INTEGER NOT NULL
);

CREATE TABLE usage_daily (
  tenant_id TEXT NOT NULL,
  day       TEXT NOT NULL,                                 -- YYYY-MM-DD in UTC
  metric    TEXT NOT NULL
            CHECK (metric IN ('inbound','outbound','sends','triage','search','agentic','ai_neurons',
                              'storage_bytes','assertions','http_signatures','cf_recipients')),
  value     INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, day, metric)
);

CREATE TABLE audit_log (
  id           TEXT PRIMARY KEY,                           -- aud_
  tenant_id    TEXT,
  actor_key_id TEXT,                                       -- the API key that acted, if any
  actor_user_id TEXT,                                      -- usr_: the person, for console actions
  action       TEXT NOT NULL,                              -- e.g. key.create, quarantine.release
  target_type  TEXT,
  target_id    TEXT,
  details_json TEXT,                                       -- never message content or clear addresses
  request_id   TEXT,
  created_at   INTEGER NOT NULL
);
CREATE INDEX audit_log_tenant ON audit_log(tenant_id, created_at DESC);
CREATE INDEX audit_log_actor  ON audit_log(actor_key_id, created_at DESC);   -- ?actor_key_id= filter (J6)
CREATE INDEX audit_log_action_target ON audit_log(action, target_id, created_at DESC);
                                     -- alert state, master_key.* and auto-suspend lookups (Observability § 5.4)

-- Thread-token, signed-link and search-cursor keys, and the Web Bot Auth deployment key. Generated by the
-- Worker (32 bytes from platform::Rng), never by the operator, and never readable through any API or the
-- CLI. Sealed under PM_MASTER_KEY in the encryption envelope of Security § 7.2 with associated data
-- "pm1|signing_keys|ciphertext|{purpose}:{kid}".
CREATE TABLE signing_keys (
  purpose      TEXT NOT NULL CHECK (purpose IN ('thread','link','cursor','web_bot_auth')),
  kid          TEXT NOT NULL,                              -- thread, link, cursor: one Crockford base32 character,
                                                           -- lower case; web_bot_auth: the RFC 7638 thumbprint
  ciphertext   BLOB NOT NULL,                              -- pm1.{kid}.{nonce}.{ciphertext} as UTF-8 bytes
                                                           -- (web_bot_auth: the sealed 32-byte Ed25519 seed)
  public_jwk   TEXT,                                       -- web_bot_auth only: the public JWK the directory lists
  created_at   INTEGER NOT NULL,
  verify_until INTEGER,                                    -- NULL for the current key of its purpose
  PRIMARY KEY (purpose, kid),
  CHECK ((purpose = 'web_bot_auth' AND length(kid) = 43 AND public_jwk IS NOT NULL)
      OR (purpose <> 'web_bot_auth' AND length(kid) = 1 AND public_jwk IS NULL))
);
CREATE UNIQUE INDEX signing_keys_current ON signing_keys(purpose) WHERE verify_until IS NULL;

-- Dead-letter records (Observability § 8). Written by the dead-letter consumers, read and redriven
-- through GET /v1/platform/dlq and POST /v1/platform/dlq/{id}/redrive (platform:ops).
CREATE TABLE dlq_items (
  id            TEXT PRIMARY KEY,                          -- dlq_
  queue         TEXT NOT NULL
                CHECK (queue IN ('pm-inbound','pm-outbound','pm-delivery-events','pm-webhooks','pm-index',
                                 'pm-screen')),
  message_id    TEXT NOT NULL,                             -- Cloudflare queue message id
  body_json     TEXT NOT NULL,                             -- the pointer as received (bodies stay under 4 KB)
  body_sha256   TEXT NOT NULL,
  tenant_id     TEXT,                                      -- named by the body, if any
  kind          TEXT,                                      -- the body's "kind", if any
  first_seen_at INTEGER NOT NULL,
  redriven_at   INTEGER,
  redrive_count INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX dlq_items_message ON dlq_items(queue, message_id);
CREATE INDEX dlq_items_open ON dlq_items(queue, first_seen_at) WHERE redriven_at IS NULL;

-- Exactly-once ingestion of mail received through Amazon SES (Domains on any DNS host § 4.5). The SNS push
-- and the SQS backstop both INSERT OR IGNORE here; only an inserted row enqueues an InboundPointer.
CREATE TABLE ses_ingest (
  object_key  TEXT NOT NULL,                               -- S3 key under in/ (= SES mail.messageId)
  recipient   TEXT NOT NULL,                               -- normalised envelope recipient
  received_at INTEGER NOT NULL,
  status      TEXT NOT NULL CHECK (status IN ('queued','held','done','dropped','lost')),
  done_at     INTEGER,                                     -- set with any terminal status (done, dropped, lost);
                                                           -- read by the global retention job (30 days after)
  PRIMARY KEY (object_key, recipient),
  CHECK ((status IN ('queued','held')) = (done_at IS NULL))
);
CREATE INDEX ses_ingest_pending ON ses_ingest(status, received_at) WHERE status IN ('queued','held');

-- Nightly vector reconciliation (Search § 6.6): one row per identity and run, written by that identity's
-- Reconcile job (INSERT OR REPLACE, so a queue retry does not count twice), plus one summary row per run
-- (identity_id = '*') written by the */15 cron. Read by the cron's drift check; rows older than 7 days are
-- deleted by the same cron.
CREATE TABLE index_reconcile (
  run_date      TEXT NOT NULL,                             -- YYYY-MM-DD (UTC) of the 02:00 run
  identity_id   TEXT NOT NULL,                             -- idn_, or '*' for the run's summary row
  embedded_rows INTEGER NOT NULL DEFAULT 0,                -- chunks rows with status 'embedded' ('*': the sum)
  pending_rows  INTEGER NOT NULL DEFAULT 0,
  failed_rows   INTEGER NOT NULL DEFAULT 0,
  queued        INTEGER,                                   -- '*' only: Reconcile jobs queued for the run
  index_count   INTEGER,                                   -- '*' only: describe() vector count when evaluated
  drift_pct     REAL,                                      -- '*' only: (index_count − embedded_rows) / embedded_rows
                                                           -- × 100; NULL until evaluated, or when not every
                                                           -- identity reported
  reported_at   INTEGER NOT NULL,
  PRIMARY KEY (run_date, identity_id)
);

-- ---------- Console: people, workspaces membership, sessions ----------
CREATE TABLE users (
  id                    TEXT PRIMARY KEY,                  -- usr_
  email                 TEXT NOT NULL UNIQUE,              -- lower case; the sign-in address
  name                  TEXT,
  status                TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','disabled')),
  last_tenant_id        TEXT,                              -- workspace used last (Cloud sign-up § 7)
  terms_version         TEXT,                              -- PM_TERMS_VERSION accepted at sign-up; shown on
  terms_accepted_at     INTEGER,                           -- /console/settings (Console § Screens)
  totp_sealed           BLOB,                              -- pm1 envelope of the 20-byte TOTP secret
  totp_enabled_at       INTEGER,
  totp_last_step        INTEGER,                           -- last accepted time step, against replay
  recovery_codes_sealed BLOB,                              -- pm1 envelope of [{ "hash": SHA-256(code), "used_at": null }]
  totp_window_start     INTEGER,                           -- start of the current one-minute attempt window
  totp_window_count     INTEGER NOT NULL DEFAULT 0,        -- two-step attempts in that window (at most 5)
  totp_failures         INTEGER NOT NULL DEFAULT 0,        -- failed codes in a row; 10 sets totp_locked_until
  totp_locked_until     INTEGER,                           -- two-step sign-in locked until (15 minutes)
  totp_pending_sealed   BLOB,                              -- enrolment: pm1 envelope of the candidate secret
  totp_pending_expires_at INTEGER,                         -- enrolment: refused after this (10 minutes); both
                                                           -- cleared on confirm, and by the retention job
  created_at            INTEGER NOT NULL,
  last_login_at         INTEGER
);

CREATE TABLE members (
  tenant_id   TEXT NOT NULL REFERENCES tenants(id),
  user_id     TEXT NOT NULL REFERENCES users(id),
  role        TEXT NOT NULL CHECK (role IN ('owner','admin','member','viewer')),
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, user_id)
);
CREATE UNIQUE INDEX members_one_owner ON members(tenant_id) WHERE role = 'owner';
CREATE INDEX members_user ON members(user_id);

CREATE TABLE invitations (
  id          TEXT PRIMARY KEY,                            -- inv_
  tenant_id   TEXT NOT NULL REFERENCES tenants(id),
  email       TEXT NOT NULL,
  role        TEXT NOT NULL CHECK (role IN ('admin','member','viewer')),
  token_hash  TEXT NOT NULL UNIQUE,                        -- HMAC(link key {key_kid}, token)
  key_kid     TEXT NOT NULL,                               -- signing_keys kid (purpose 'link') of token_hash
  invited_by  TEXT REFERENCES users(id),                   -- the console user who invited; NULL for an API key.
                                                           -- Read by GET …/members and the invitation email
  status      TEXT NOT NULL CHECK (status IN ('pending','accepted','revoked','expired')),
  expires_at  INTEGER NOT NULL,                            -- created + 7 days
  created_at  INTEGER NOT NULL
);
CREATE UNIQUE INDEX invitations_pending ON invitations(tenant_id, email) WHERE status = 'pending';

CREATE TABLE login_tokens (                                -- magic links and six-digit codes
  id          TEXT PRIMARY KEY,
  email       TEXT NOT NULL,
  purpose     TEXT NOT NULL CHECK (purpose IN ('sign_in','sign_up','waitlist','oauth_link')),
                                                           -- what using it does (Cloud sign-up § 6);
                                                           -- re-authentication is sign_in; oauth_link confirms
                                                           -- a Google or GitHub link (Cloud sign-up § 4)
  plan        TEXT,                                        -- sign_up: plan intent; waitlist: plan of interest
  next_path   TEXT,                                        -- sign_up: validated next (Cloud sign-up § 7)
  terms_version TEXT,                                      -- sign_up: PM_TERMS_VERSION accepted; copied to users
                                                           -- with terms_accepted_at = created_at
  token_hash  TEXT NOT NULL UNIQUE,                        -- HMAC(link key {key_kid}, link token)
  code_hash   TEXT NOT NULL,                               -- HMAC(link key {key_kid}, email || code)
  key_kid     TEXT NOT NULL,                               -- signing_keys kid (purpose 'link')
  attempts    INTEGER NOT NULL DEFAULT 0,                  -- ≤ 10, then the token is burned
  expires_at  INTEGER NOT NULL,                            -- created + 10 minutes
  used_at     INTEGER,
  created_at  INTEGER NOT NULL
);
CREATE INDEX login_tokens_email ON login_tokens(email, created_at);

CREATE TABLE sessions (
  id_hash         TEXT PRIMARY KEY,                        -- HMAC(link key {key_kid}, cookie value)
  key_kid         TEXT NOT NULL,                           -- signing_keys kid (purpose 'link') of id_hash
  user_id         TEXT NOT NULL REFERENCES users(id),
  tenant_id       TEXT REFERENCES tenants(id),             -- active workspace
  csrf_secret     TEXT NOT NULL,
  authenticated_at INTEGER NOT NULL,                       -- for "signed in within 10 minutes" checks
  last_seen_at    INTEGER NOT NULL,
  expires_at      INTEGER NOT NULL,                        -- rolling 7 days, absolute 30 days
  revoked_at      INTEGER,
  user_agent_hint TEXT                                     -- browser family only
);
CREATE INDEX sessions_user ON sessions(user_id);

-- Google and GitHub sign-in (Cloud sign-up § 4). A verified provider address links to the existing user.
CREATE TABLE oauth_identities (
  provider      TEXT NOT NULL CHECK (provider IN ('google','github')),
  subject       TEXT NOT NULL,                             -- Google sub, GitHub numeric id
  user_id       TEXT NOT NULL REFERENCES users(id),
  email_at_link TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  last_used_at  INTEGER,
  PRIMARY KEY (provider, subject)
);
CREATE INDEX oauth_identities_user ON oauth_identities(user_id);

CREATE TABLE oauth_states (                                -- one row per started OAuth flow, single use
  state_hash  TEXT PRIMARY KEY,                            -- HMAC(link key {key_kid}, state)
  cookie_hash TEXT NOT NULL,                               -- HMAC(link key {key_kid}, __Host-pm_oauth value)
  key_kid     TEXT NOT NULL,                               -- signing_keys kid (purpose 'link') of both hashes
  provider    TEXT NOT NULL CHECK (provider IN ('google','github')),
  intent      TEXT NOT NULL CHECK (intent IN ('sign_in','sign_up')),  -- read by the callback (§ 4 step 5)
  pkce_sealed BLOB NOT NULL,                               -- pm1 envelope of the PKCE verifier
  nonce       TEXT,                                        -- Google only
  next_path   TEXT,                                        -- validated next (Cloud sign-up § 7)
  plan        TEXT,
  terms_version TEXT,                                      -- intent sign_up: PM_TERMS_VERSION accepted at the start;
                                                           -- copied to the new user by the callback
  invitation_id TEXT REFERENCES invitations(id),           -- set when the flow started on an invitation's accept
                                                           -- page; the callback accepts that invitation only
  created_at  INTEGER NOT NULL,
  expires_at  INTEGER NOT NULL,                            -- created + 10 minutes
  used_at     INTEGER
);

-- A first factor passed and a second step is outstanding (Cloud sign-up § 5.1): no session exists yet.
CREATE TABLE pending_auth (
  id_hash        TEXT PRIMARY KEY,                         -- HMAC(link key {key_kid}, __Host-pm_pending value)
  key_kid        TEXT NOT NULL,                            -- signing_keys kid (purpose 'link') of id_hash
  user_id        TEXT NOT NULL REFERENCES users(id),
  step           TEXT NOT NULL CHECK (step IN ('link_code','two_factor')),
  next_path      TEXT,                                     -- validated next (Cloud sign-up § 7)
  plan           TEXT,                                     -- sign-up plan intent, carried to § 7
  invitation_id  TEXT REFERENCES invitations(id),          -- accepted, in the batch that creates the session
  oauth_provider TEXT CHECK (oauth_provider IN ('google','github')),  -- link_code: the identity to link
  oauth_subject  TEXT,
  oauth_email    TEXT,                                     -- becomes oauth_identities.email_at_link
  login_token_id TEXT REFERENCES login_tokens(id),         -- link_code: the emailed code (purpose oauth_link)
  agent_signup_id TEXT REFERENCES agent_signups(id) ON DELETE CASCADE,  -- two_factor step of an agent sign-up
                                                           -- link (Agent self-sign-up § 7.2)
  attempts       INTEGER NOT NULL DEFAULT 0,               -- wrong link codes; at 5 the row is used up
  created_at     INTEGER NOT NULL,
  expires_at     INTEGER NOT NULL,                         -- two_factor: + 5 minutes; link_code: + 10 minutes
  used_at        INTEGER                                   -- single use
);
CREATE INDEX pending_auth_user ON pending_auth(user_id);

CREATE TABLE waitlist (                                    -- PM_SIGNUP = waitlist (Cloud sign-up § 6.1)
  email        TEXT PRIMARY KEY,                           -- needed to send the invitation; deleted as in the notes
  plan         TEXT,                                       -- plan of interest
  created_at   INTEGER NOT NULL,
  confirmed_at INTEGER NOT NULL,                           -- double opt-in link used: rows exist only once
                                                           -- confirmed; invites go oldest confirmed_at first
  invited_at   INTEGER,                                    -- invite link sent (GET /console/sign-up?invite=…),
                                                           -- valid 7 days while PM_SIGNUP = waitlist
  invite_token_hash TEXT UNIQUE,                           -- HMAC(link key {key_kid}, sign-up link token)
  key_kid      TEXT                                        -- signing_keys kid (purpose 'link') of invite_token_hash
);

-- One row per agent sign-up (Agent self-sign-up § 15). The clear human address is kept only while the sign-up
-- is provisional; the row is deleted by the tenant-scope erasure of its tenant.
CREATE TABLE agent_signups (
  id                TEXT PRIMARY KEY,                        -- asu_
  tenant_id         TEXT NOT NULL UNIQUE REFERENCES tenants(id),
  identity_id       TEXT NOT NULL REFERENCES identities(id),  -- the 'agent' identity
  key_id            TEXT NOT NULL REFERENCES api_keys(id),    -- the key returned at sign-up
  state             TEXT NOT NULL
                    CHECK (state IN ('provisional','verified','expired','abandoned','refused')),
  human_email       TEXT,                                    -- normalised; NULL once the state leaves provisional
  human_hash        TEXT NOT NULL,                           -- hex HMAC-SHA256(PM_HASH_KEY, base address)
  human_status      TEXT NOT NULL DEFAULT 'pending'
                    CHECK (human_status IN ('pending','delivered','bounced')),  -- delivery hook (§ 13.3)
  replacements_used INTEGER NOT NULL DEFAULT 0 CHECK (replacements_used BETWEEN 0 AND 2),
  code_seq          INTEGER NOT NULL DEFAULT 1,              -- +1 with every new code (resend, replacement)
  code_hash         TEXT,                                    -- HMAC(link key {key_kid}, "asu:{id}:{code_seq}:{code}");
                                                             -- NULL when burned, used or ended
  link_hash         TEXT UNIQUE,                             -- HMAC(link key {key_kid}, link token)
  key_kid           TEXT,                                    -- signing_keys kid (purpose 'link') of both hashes
  code_expires_at   INTEGER,                                 -- code and link: issued + 24 hours
  attempts          INTEGER NOT NULL DEFAULT 0,              -- reserved tries of the current code (≤ 10)
  failed_total      INTEGER NOT NULL DEFAULT 0,              -- wrong codes over the sign-up's life (≤ 30)
  sends_json        TEXT NOT NULL DEFAULT '[]',              -- times of the code emails of the last hour (≤ 3)
  mail_message_ids_json TEXT NOT NULL DEFAULT '[]',          -- msg_ IDs of the code emails (system identity)
  mail_status       TEXT NOT NULL DEFAULT 'queued'
                    CHECK (mail_status IN ('queued','not_sent')),  -- the latest code email
  source            TEXT,                                    -- [A-Za-z0-9._/+-]{1,64}
  network_hash      TEXT NOT NULL,                           -- 16 hex: HMAC(PM_HASH_KEY, IPv4 or IPv6 /64)
  terms_version     TEXT NOT NULL,                           -- PM_TERMS_VERSION accepted by the caller
  expires_at        INTEGER NOT NULL,                        -- created_at + 7 days; equals tenants.provisional_until
  verified_at       INTEGER,
  verified_via      TEXT CHECK (verified_via IN ('code','link','claim')),
  verified_user_id  TEXT REFERENCES users(id),
  verify_token      TEXT,                                    -- the winner's nonce guarding the verification batch
  ended_at          INTEGER,                                 -- expired, abandoned or refused
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE UNIQUE INDEX agent_signups_live_human ON agent_signups(human_hash) WHERE state = 'provisional';
CREATE INDEX agent_signups_expiry ON agent_signups(expires_at) WHERE state = 'provisional';
CREATE INDEX agent_signups_created ON agent_signups(created_at);         -- the platform listing

-- People who stopped agents naming them (Agent self-sign-up § 9). Keyed hash only; kept as an objection to
-- contact.
CREATE TABLE agent_signup_refusals (
  human_hash TEXT PRIMARY KEY,                               -- hex HMAC-SHA256(PM_HASH_KEY, base address)
  reason     TEXT NOT NULL CHECK (reason IN ('refused','complaint')),
  created_at INTEGER NOT NULL
);

-- Notification preferences of a person in one workspace (Notifications § 2). A missing row means the default:
-- usage = instant and needs_person = daily for owners and admins, off for members and viewers; new_mail = off.
-- Written only by the console settings page, the one-click unsubscribe and the bounce handling; read by the
-- Notifier (cached, meta.prefs_cache_at) and by the webhook dispatcher's "any new_mail preference" check.
CREATE TABLE notification_prefs (
  user_id       TEXT NOT NULL REFERENCES users(id),
  tenant_id     TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('usage','new_mail','needs_person')),
  mode          TEXT NOT NULL CHECK (mode IN ('off','instant','hourly','daily')),  -- usage: off|instant;
                                                                                   -- needs_person: off|daily
  filter        TEXT NOT NULL DEFAULT 'all' CHECK (filter IN ('all','needs_reply')), -- new_mail only
  identity_ids  TEXT,                                      -- JSON array; NULL = every inbox (new_mail only)
  paused_reason TEXT CHECK (paused_reason IN ('bounce','complaint')),  -- set on every row of the person by a
                                                           -- hard bounce or complaint on a notification; cleared
                                                           -- when they confirm their address in the console
  updated_at    INTEGER NOT NULL,
  PRIMARY KEY (user_id, tenant_id, kind)
);
CREATE INDEX notification_prefs_tenant ON notification_prefs(tenant_id, kind);

-- ---------- Plans and billing ----------
CREATE TABLE billing_accounts (
  tenant_id            TEXT PRIMARY KEY REFERENCES tenants(id),
  mode                 TEXT NOT NULL CHECK (mode IN ('metered','exempt','disabled')),
  plan_id              TEXT NOT NULL DEFAULT 'free',       -- key into PM_PLAN_CATALOG
  status               TEXT NOT NULL DEFAULT 'active'
                       CHECK (status IN ('active','trialing','past_due','canceled','incomplete')),
  topups_json          TEXT NOT NULL DEFAULT '{}',         -- {"inboxes":2,"sends":5,"triage":0} units
  period_start         INTEGER NOT NULL,                   -- current allowance period
  period_end           INTEGER NOT NULL,
  grace_until          INTEGER,                            -- past_due grace end (7 days)
  stripe_customer_id   TEXT UNIQUE,
  stripe_subscription_id TEXT UNIQUE,                     -- the plan subscription; written by Applying state,
                                                           -- read by the plan_managed_by_stripe check (Billing)
  topup_subscriptions_json TEXT NOT NULL DEFAULT '{}',    -- {"sends":"sub_…"}: live top-up subscriptions by
                                                           -- feature; written by Applying state, read by the
                                                           -- console's Portal deep links (Billing)
  dispute_open_at      INTEGER,                            -- a charge of the customer is disputed: default plan
                                                           -- and no sends until the dispute closes (Billing ›
                                                           -- Disputes and refunds); read by outbound step 18
  cancel_at_period_end INTEGER NOT NULL DEFAULT 0,
  scheduled_plan_id    TEXT,                               -- a downgrade waiting for the period end and when it
  scheduled_plan_at    INTEGER,                            -- starts; written by Applying state from the plan
                                                           -- subscription's schedule, read by the console's
                                                           -- plan page (Billing › Plan changes)
  updated_at           INTEGER NOT NULL
);

CREATE TABLE billing_events (                              -- Stripe webhook deduplication and audit
  id           TEXT PRIMARY KEY,                           -- Stripe event id (evt_…)
  type         TEXT NOT NULL,
  tenant_id    TEXT,
  received_at  INTEGER NOT NULL,
  processed_at INTEGER,
  outcome      TEXT                                        -- applied | ignored_stale | ignored_erased | ignored_unresolved
                                                           -- | cancelled_after_erasure | error:<code>
               CHECK (outcome IN ('applied','ignored_stale','ignored_erased','ignored_unresolved',
                                  'cancelled_after_erasure') OR outcome LIKE 'error:%')
);

-- Deployment-wide switches written by crons (Cloud sign-up § 10.3).
CREATE TABLE platform_state (
  key        TEXT PRIMARY KEY CHECK (key IN ('send_breaker','phone_account','alert_evaluator')),
  value_json TEXT NOT NULL,                                -- send_breaker: {"stage": 1 | 2, "until": <ms>, "share": <0..1>}
                                                           -- phone_account: {"level": "paid" | "verified" | "unknown",
                                                           -- "probed_at": <ms>, "us_enabled": bool}, written by
                                                           -- POST /v1/platform/phone/probe (Phone numbers § 2.1)
                                                           -- alert_evaluator: {"run", "last_run_at", "keys": {alert key:
                                                           -- {severity, fired_at, notified_at, notices, false_runs,
                                                           -- tenant_id}}}, the evaluator's memory between runs
                                                           -- (Observability § 5.4)
  updated_at INTEGER NOT NULL
);

CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);

-- Deployment-wide operator switches (Observability § 5.6). Written by PUT /v1/platform/switches
-- (platform:ops) and by the automatic containment rules; read, with a 60-second per-isolate cache, by the
-- request router (read_only), the outbound submit and consumer (read_only, free_sending) and the tenant
-- retention job (emergency_prune); shown by GET /v1/platform/status. A missing row means the default.
CREATE TABLE ops_switches (
  name       TEXT PRIMARY KEY CHECK (name IN ('read_only','free_sending','emergency_prune','agent_signup',
                                          'phone','vault_redeem')),
  state      TEXT NOT NULL CHECK (state IN ('on','off','paused_purchases','paused')),
                                                           -- defaults: read_only off, free_sending on,
                                                           -- emergency_prune off, agent_signup on, phone on,
                                                           -- vault_redeem on. paused_purchases and paused are
                                                           -- the phone switch's only (Phone numbers § 30)
  set_by     TEXT NOT NULL CHECK (set_by IN ('auto','platform')),
  reason     TEXT NOT NULL,                                -- the containment rule, or the operator's note
  actor_key_id TEXT,                                       -- the platform key, when set_by = 'platform'
  set_at     INTEGER NOT NULL
);

-- Deployment-wide Durable Objects whose IDs must be minted inside the Worker (jurisdiction).
CREATE TABLE platform_objects (
  name       TEXT PRIMARY KEY CHECK (name IN ('ses_control')),
  do_id      TEXT NOT NULL,                                -- minted by the every-minute cron
  created_at INTEGER NOT NULL
);
```

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

```sql
-- The tenant vault key (Vault § 4.2): one active X25519 key per tenant, a retiring one during rotation.
-- Written by the first vault write, rotate-key, custody changes and erasure; read when a DEK is sealed
-- (public key) or unwrapped (private key: mint, sign, reveal, rotation).
CREATE TABLE vault_keys (
  tenant_id      TEXT NOT NULL REFERENCES tenants(id),
  kid            TEXT NOT NULL,                             -- RFC 7638 thumbprint of the public JWK
  public_key     BLOB NOT NULL,                             -- 32 bytes
  private_sealed BLOB,                                      -- pm1 envelope, aad pm1|vault_keys|private_sealed|{tenant_id}:{kid};
                                                            -- NULL in partner custody
  custody        TEXT NOT NULL CHECK (custody IN ('operator','partner')),
  state          TEXT NOT NULL CHECK (state IN ('active','retiring')),
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, kid)
);
CREATE UNIQUE INDEX vault_keys_one_active ON vault_keys(tenant_id) WHERE state = 'active';

-- Partner custody slots (Vault § 4.6): the tenant private key wrapped under an unlock key, then sealed under
-- the master key. Written by enrol and release; read on unwrapping routes; deleted first by tenant erasure.
CREATE TABLE vault_custody_slots (
  tenant_id       TEXT NOT NULL REFERENCES tenants(id),
  kid             TEXT NOT NULL,
  slot            INTEGER NOT NULL CHECK (slot IN (1,2)),
  private_wrapped BLOB NOT NULL,                            -- pm1( AES-256-GCM(unlock key, private, aad pvu1|{tenant_id}|{kid}|{slot}) )
  created_at      INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, kid, slot)
);

-- Vault items (Vault § 3.1). Values are never here: they are in vault_item_versions or R2.
CREATE TABLE vault_items (
  id                TEXT PRIMARY KEY,                       -- vit_
  tenant_id         TEXT NOT NULL REFERENCES tenants(id),
  identity_id       TEXT REFERENCES identities(id),         -- NULL: tenant-scoped
  kind              TEXT NOT NULL CHECK (kind IN ('login','api_key','session','managed_secret')),
  title             TEXT NOT NULL,                          -- 1–120 chars, untrusted
  account_id        TEXT REFERENCES service_accounts(id),   -- required for identity-scoped items, except managed
  username          TEXT,
  origins_json      TEXT NOT NULL,                          -- 1–10 canonical origins
  approval          TEXT NOT NULL CHECK (approval IN ('grant','per_run','per_use')),
  state             TEXT NOT NULL CHECK (state IN ('active','archived')),
  health            TEXT NOT NULL CHECK (health IN ('ok','needs_auth','needs_attention','needs_rotation')),
  health_reason     TEXT,
  current_version   INTEGER NOT NULL,
  has_json          TEXT NOT NULL,                          -- which sealed fields exist; never values
  otp_channel_json  TEXT,                                   -- login only
  inject_json       TEXT,                                   -- api_key only: scheme, header_name, username
  sign_json         TEXT,                                   -- api_key only, P1
  rotate_after_use  INTEGER NOT NULL DEFAULT 0,
  rotation_json     TEXT NOT NULL,                          -- remind_after_days, last_rotated_at
  session_json      TEXT,                                   -- session only: ttl_seconds, expires_at, size_bytes, counts
  totp_last_step    INTEGER,                                -- one code per step per item (§ 9.1)
  managed_by        TEXT CHECK (managed_by IN ('phone')),
  owner_ref         TEXT,                                   -- managed only: the owning service's resource ID
  managed_purpose   TEXT CHECK (managed_purpose IN ('port_out_pin')),
  created_by_key_id TEXT,
  created_by_user_id TEXT,
  first_used_at     INTEGER,
  last_used_at      INTEGER,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE INDEX vault_items_identity ON vault_items(identity_id, state, created_at);
CREATE INDEX vault_items_tenant ON vault_items(tenant_id, state, created_at);
CREATE INDEX vault_items_account ON vault_items(account_id);
CREATE UNIQUE INDEX vault_items_managed ON vault_items(tenant_id, owner_ref, managed_purpose) WHERE managed_by IS NOT NULL;

-- Item versions (Vault § 4.3, § 11.1): at most current, previous and pending per item.
CREATE TABLE vault_item_versions (
  item_id          TEXT NOT NULL REFERENCES vault_items(id),
  version          INTEGER NOT NULL,
  role             TEXT NOT NULL CHECK (role IN ('current','previous','pending')),
  tvk_kid          TEXT NOT NULL,                           -- the tenant key the DEK is sealed to
  dek_sealed       BLOB NOT NULL,                           -- HPKE enc ‖ ciphertext, aad pv1|dek|{tenant}|{item}|{version}|{kid}
  fields_json      TEXT,                                    -- {purpose: "pv1.{nonce}.{ct}"}; NULL for sessions
  r2_sha256        TEXT,                                    -- sessions: SHA-256 of the R2 ciphertext
  overlap_until    INTEGER,                                 -- previous only
  expires_at       INTEGER,                                 -- sessions only
  import_source    TEXT,                                    -- pylota_sealed_session
  created_at       INTEGER NOT NULL,
  PRIMARY KEY (item_id, version)
);
CREATE UNIQUE INDEX vault_item_versions_role ON vault_item_versions(item_id, role);
CREATE INDEX vault_item_versions_kid ON vault_item_versions(tvk_kid);

-- Grants (Vault § 5).
CREATE TABLE vault_grants (
  id                  TEXT PRIMARY KEY,                     -- vgr_
  tenant_id           TEXT NOT NULL REFERENCES tenants(id),
  item_id             TEXT NOT NULL REFERENCES vault_items(id),
  identity_id         TEXT NOT NULL REFERENCES identities(id),
  operations_json     TEXT NOT NULL,
  origins_json        TEXT NOT NULL,
  status              TEXT NOT NULL CHECK (status IN ('pending_approval','active','denied','revoked','expired')),
  purpose             TEXT NOT NULL,                        -- untrusted
  max_uses            INTEGER,
  use_count           INTEGER NOT NULL DEFAULT 0,
  requested_by_key_id TEXT,
  decided_by_key_id   TEXT,
  decided_by_user_id  TEXT,
  on_behalf_of        TEXT,
  note                TEXT,
  decided_at          INTEGER,
  expires_at          INTEGER NOT NULL,
  revoked_at          INTEGER,
  revoke_reason       TEXT,
  created_at          INTEGER NOT NULL,
  updated_at          INTEGER NOT NULL
);
CREATE INDEX vault_grants_identity ON vault_grants(identity_id, status, created_at);
CREATE INDEX vault_grants_item ON vault_grants(item_id, status);
CREATE INDEX vault_grants_tenant ON vault_grants(tenant_id, status, created_at);

-- Uses (Vault § 6, § 7). dek_ticket_sealed is cleared when the use ends; ticket_hash at ticket_valid_until (§ 4.4).
CREATE TABLE vault_uses (
  id                 TEXT PRIMARY KEY,                      -- vus_
  tenant_id          TEXT NOT NULL REFERENCES tenants(id),
  identity_id        TEXT NOT NULL REFERENCES identities(id),
  item_id            TEXT NOT NULL REFERENCES vault_items(id),
  grant_id           TEXT NOT NULL REFERENCES vault_grants(id),
  consumer_id        TEXT REFERENCES vault_consumers(id),   -- NULL for sign
  operation          TEXT NOT NULL CHECK (operation IN ('fill','rotate_fill','session_load','session_capture','inject','sign')),
  phases_json        TEXT NOT NULL,
  phases_redeemed_json TEXT NOT NULL DEFAULT '[]',
  run_id             TEXT NOT NULL,
  origin             TEXT NOT NULL,
  trigger            TEXT NOT NULL,
  purpose            TEXT NOT NULL,                         -- untrusted
  capture_into       TEXT REFERENCES vault_items(id),
  run_window_seconds INTEGER,
  version_choice     TEXT NOT NULL DEFAULT 'current' CHECK (version_choice IN ('current','previous')),
  status             TEXT NOT NULL CHECK (status IN ('pending_approval','approved','in_use','closed','denied','expired','canceled')),
  approval_reason    TEXT,
  approved_by        TEXT CHECK (approved_by IN ('person','key','run','grant')),
  run_approval_until INTEGER,
  digest             TEXT NOT NULL,
  deadline_at        INTEGER,
  ticket_lookup      TEXT,
  ticket_hash        TEXT,                                  -- hex SHA-256 of the ticket
  ticket_version     INTEGER,
  dek_ticket_sealed  BLOB,                                  -- AES-256-GCM under the ticket secret
  ticket_issued_at   INTEGER,
  redeem_by          INTEGER,
  first_redeemed_at  INTEGER,
  closes_at          INTEGER,
  closed_at          INTEGER,
  ticket_valid_until INTEGER,                               -- reports and allowed write-backs authenticate until then
  close_reason       TEXT,
  outcome            TEXT,
  write_back_done    INTEGER NOT NULL DEFAULT 0,
  created_by_key_id  TEXT NOT NULL,
  decided_by_key_id  TEXT,
  decided_by_user_id TEXT,
  on_behalf_of       TEXT,
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);
CREATE UNIQUE INDEX vault_uses_ticket ON vault_uses(ticket_lookup) WHERE ticket_lookup IS NOT NULL;
CREATE INDEX vault_uses_identity ON vault_uses(identity_id, status, created_at);
CREATE INDEX vault_uses_tenant ON vault_uses(tenant_id, status, created_at);
CREATE INDEX vault_uses_item_open ON vault_uses(item_id) WHERE status IN ('pending_approval','approved','in_use');
CREATE INDEX vault_uses_deadlines ON vault_uses(status, deadline_at, redeem_by, closes_at);

-- Consumers (Vault § 8.1, § 8.2).
CREATE TABLE vault_consumers (
  id                   TEXT PRIMARY KEY,                    -- vcn_
  kind                 TEXT NOT NULL CHECK (kind IN ('browser','egress_proxy','executor')),
  name                 TEXT NOT NULL,
  level                TEXT NOT NULL CHECK (level IN ('platform','partner','tenant')),
  partner_id           TEXT REFERENCES partners(id),
  tenant_id            TEXT REFERENCES tenants(id),
  hosting              TEXT NOT NULL CHECK (hosting IN ('self_hosted','third_party')),
  provider             TEXT,
  contract             TEXT NOT NULL,
  signing_jwk_json     TEXT,                                -- cleared on revoke
  signing_kid          TEXT,
  prev_signing_jwk_json TEXT,
  prev_signing_kid     TEXT,
  prev_signing_until   INTEGER,
  encryption_jwk_json  TEXT,
  encryption_kid       TEXT,
  declarations_json    TEXT NOT NULL,
  status               TEXT NOT NULL CHECK (status IN ('pending_activation','active','suspended','revoked')),
  activation_note      TEXT,
  activated_by_key_id  TEXT,
  activated_by_user_id TEXT,
  activated_at         INTEGER,
  revoked_at           INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);
CREATE UNIQUE INDEX vault_consumers_signing_kid ON vault_consumers(signing_kid) WHERE signing_kid IS NOT NULL;
CREATE INDEX vault_consumers_prev_kid ON vault_consumers(prev_signing_kid) WHERE prev_signing_kid IS NOT NULL;
CREATE INDEX vault_consumers_scope ON vault_consumers(level, partner_id, tenant_id, status);

-- Consumer signature nonces (Vault § 8.4): kept until 60 s after the signature's expires.
CREATE TABLE vault_consumer_nonces (
  consumer_id TEXT NOT NULL,
  nonce       TEXT NOT NULL,
  expires_at  INTEGER NOT NULL,
  PRIMARY KEY (consumer_id, nonce)
);
CREATE INDEX vault_consumer_nonces_expiry ON vault_consumer_nonces(expires_at);

-- The vault audit (Vault § 14): append-only, hash-chained, per tenant; deleted only by age (400 days) and erasure.
CREATE TABLE vault_audit (
  id                TEXT PRIMARY KEY,                       -- vad_
  tenant_id         TEXT NOT NULL,
  seq               INTEGER NOT NULL,
  prev_hash         TEXT NOT NULL,
  row_hash          TEXT NOT NULL,
  action            TEXT NOT NULL,
  item_id           TEXT,
  version           INTEGER,
  grant_id          TEXT,
  use_id            TEXT,
  consumer_id       TEXT,
  identity_id       TEXT,
  actor_key_id      TEXT,
  actor_user_id     TEXT,
  actor_consumer_id TEXT,
  on_behalf_of      TEXT,
  operation         TEXT,
  phase             TEXT,
  outcome           TEXT,
  reason            TEXT,
  site              TEXT,                                   -- registrable domain, never a URL
  details_json      TEXT NOT NULL DEFAULT '{}',             -- counts and enumerations only
  created_at        INTEGER NOT NULL
);
CREATE UNIQUE INDEX vault_audit_seq ON vault_audit(tenant_id, seq);
CREATE INDEX vault_audit_item ON vault_audit(tenant_id, item_id, seq);
CREATE INDEX vault_audit_created ON vault_audit(created_at);

-- The run of each audit row (Vault § 14.1), outside row_hash, so identity erasure can delete it
-- without changing the chain.
CREATE TABLE vault_audit_runs (
  audit_id   TEXT PRIMARY KEY REFERENCES vault_audit(id) ON DELETE CASCADE,
  tenant_id  TEXT NOT NULL,
  run_id     TEXT NOT NULL
);
CREATE INDEX vault_audit_runs_run ON vault_audit_runs(tenant_id, run_id);

-- Audit anchors (Vault § 16.1): the last row retention deleted, so the chain stays verifiable.
CREATE TABLE vault_audit_anchors (
  tenant_id  TEXT PRIMARY KEY,
  seq        INTEGER NOT NULL,
  row_hash   TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### Notes

- **Directory lookups.** `email()` runs one query per message:

  ```sql
  SELECT a.status, a.identity_id, a.tenant_id, i.mailbox_do_id, i.status, t.status, t.mode
  FROM addresses a
  JOIN identities i ON i.id = a.identity_id
  JOIN tenants t ON t.id = a.tenant_id
  WHERE a.address = ?1
  ```

  If nothing matches, it checks `address_tombstones`, so that erased and deleted addresses get the
  same `550 5.1.1` as unknown ones.
- **Address uniqueness is global.** A retired address keeps its row, so it can never be reassigned.
  Deleting an identity moves its address rows to `address_tombstones` and removes them from `addresses`.
- **Suppressions store only a keyed hash** and a masked hint. A suppression outlives counterparty
  erasure, because it records an objection to contact (UK and EU GDPR Art. 21). The privacy guide says so.
- **Signing keys.** `signing_keys` holds the keyring for thread tokens ([Threading](threading.md#24-key-rotation)),
  signed links ([Security › Signed links](security.md#73-signed-links)) and search cursors
  ([Search › Cursors](search.md#58-cursors-and-as_of-pinning)). `link` signs download links, console
  sign-in, invitation and session tokens and OAuth state hashes; it no longer signs cursors. `cursor` signs
  search cursors and list cursors ([REST API › Pagination](../../reference/api.md#pagination)). The Worker
  creates the first key of each purpose on first use (`INSERT … ON CONFLICT DO NOTHING`, then a re-read,
  so two isolates racing end with one key).
  `POST /v1/platform/keys/{purpose}/rotate` inserts a new current key and sets `verify_until` on the old
  one: 90 days for `thread`, 7 days for `link` (the longest link lifetime: `link_ttl_hours` ≤ 168; export
  links live 1 hour), 24 hours for `cursor` (the cursor lifetime). With `?revoke_previous=true` the
  old row is deleted in the same D1 batch instead, so what it signed stops verifying at once. Rows past
  `verify_until` are deleted by the global retention job. Isolates cache the opened keyring for 5 minutes
  and re-read it at once when a token names an unknown kid (at most once a minute per isolate).
  Notification unsubscribe tokens ([Notifications § 5](notifications.md#5-the-emails)) are MACs under a
  `link` key too. They claim 90 days but verify only while their key is inside its window, so after a
  `link` rotation an older token fails (and gets the expired-token page) once its key's 7 days are over.
- **Web Bot Auth key.** The `web_bot_auth` purpose holds the deployment key of
  [Agent signing keys](agent-keys.md#2-keys): its kid is the 43-character RFC 7638 thumbprint and
  `public_jwk` holds the public key the directory lists. The Worker creates it on first use (the first
  signing request or directory fetch while `PM_WEB_BOT_AUTH=on`), with the same `ON CONFLICT DO NOTHING`
  and re-read. A rotation sets `verify_until` = now + 7 days on the old key; the directory lists the
  current key and at most the two newest keys still inside `verify_until`.
- **Identity keys.** `identity_keys` has at most one `active` row per identity
  (`identity_keys_one_active`). The Worker writes the first row on the identity's first signing request
  or on `POST …/keys`; a rotation inserts the new `active` row and sets the old one to `retiring` with
  `verify_until` in one D1 batch; a revocation, or the global retention job once `verify_until` has
  passed, sets `retired` and `retired_at`. The JWKS lists `active` rows and the `retiring` rows whose
  `verify_until` has not passed, so it never depends on that job's timing; `retired` rows are
  kept until the identity is erased, so a thumbprint is never reused, and then move to `key_tombstones`.
- **Sealed values.** These columns hold the pm1 envelope of
  [Security § 7.2](security.md#72-encryption-envelope) under `PM_MASTER_KEY`, with associated data
  `pm1|{table}|{column}|{row id}` (for example `pm1|domains|smtp_sealed|{domain_id}`):
  `webhook_endpoints.secret_enc`, `prev_secret_enc` and `headers_sealed`, `identity_keys.private_enc`,
  `signing_keys.ciphertext`, `domains.smtp_sealed` and `smtp_pending_sealed`, `users.totp_sealed`,
  `users.totp_pending_sealed`, `users.recovery_codes_sealed` and `oauth_states.pkce_sealed`. Each is an
  entry of the sealed-column
  registry (`crates/core/src/sealed.rs`), which the re-seal sweep and the count query of
  `idvy secrets rotate-master` read, so the rotation covers every one of them. Recovery codes are sealed, not hashed under a `link` key, because link keys are deleted 7 days
  after a rotation and recovery codes live for months.
- **Notification preferences.** `notification_prefs` rows exist only where a person changed a default.
  Removing a member deletes their rows for that workspace ([O19](../edge-cases.md)); erasing a person
  deletes all of theirs; tenant erasure deletes the workspace's rows. A bounce or complaint on a
  notification writes `paused_reason` on all of a person's rows, inserting the default row for a kind that
  has none ([Notifications § 5](notifications.md#5-the-emails)).
- **Columns specified by the REST API.** These columns are written only by a REST endpoint and read
  back by it or by its list filters, and no design page adds behaviour to them, so
  [REST API](../../reference/api.md) and `openapi.yaml` are their specification: `identities.purpose`
  (`POST …/identities`, `PATCH`; the `purpose` list filter), `addresses.local_part` (`POST …/addresses`;
  the Address object), `webhook_deliveries.next_attempt_at` (written by
  [Webhooks › Retry schedule](webhooks.md#retry-schedule-j4-fr-wh-3); read by
  `GET /v1/webhooks/{webhook_id}/deliveries`), `sender_lists.tenant_id`, `direction`, `kind`, `entry`,
  `note` and `created_at` (`PUT /v1/tenants/{tenant_id}/lists/{direction}/{kind}/{entry}`; read by the
  list endpoints, and `entry` by the inbound and outbound list checks; `identity_id` is written by the
  identity-scope routes and read by the same checks, [Parity additions § 3](parity-additions.md#3-identity-scope-allow-and-block-lists-fr-out-17)), `members.created_at` (the
  Member object of `GET /v1/tenants/{tenant_id}/members`) and the mailbox's `threads.archived`
  (`PATCH …/threads/{thread_id}`; the `archived` list filter).
- **Who acted.** `audit_log.actor_key_id` names the API key and `actor_user_id` the person who acted in
  the console. Actions of the Worker itself (the Stripe webhook, crons) have neither. A quarantine
  release is not stored on the message, which goes back to `received`: its actor is in the
  `quarantine.release` audit row and in the `message.released` event (`released_by_key_id`, or
  `released_by_user_id` for a console release).
- **Key provenance.** `api_keys.created_by_user_id` and `created_by_role` are written by `POST /v1/keys`:
  from the session for a console mint, and copied from the calling key otherwise
  ([Security › Who minted a key](security.md#who-minted-a-key)). `created_by_role` is read by `POST /v1/keys`
  (a tenant key may hold `tenants:erase` only when it is `owner`) and rewritten on a role change;
  `created_by_user_id` is read by member removal and role changes, which revoke the person's keys in the
  same batch (`api_keys_creator`), and returned in the API key object.
- **RPC intents.** `rpc_intents` rows are written in the D1 batch of tenant creation (`init` of the
  `TenantQuota`), identity creation (`init` of the mailbox, carrying `identity.created`), identity updates,
  pauses and resumes, tenant suspension and resumption (one `emit_event` per identity paused or resumed),
  identity-key changes (the `identity.key_*` events) and changes of an identity's `retention.message_hours`
  (`set_retention`, carrying `MailboxRequest::SetRetention`, [Parity additions § 7.2](parity-additions.md#72-how-the-mailbox-learns-it)). The handler deletes its rows once the calls
  succeed; the every-minute cron (`crons/intents.rs`) re-sends due rows in `occurred_at` order per object
  and deletes each on success, or when the object answers that it is erased
  ([Design conventions § 9](index.md#9-durable-object-calls-after-a-d1-change)). Tenant erasure deletes the
  tenant's rows with its other D1 rows.
- **Partners.** `partners` rows are written by `POST /v1/partners` and changed by
  `PATCH /v1/partners/{partner_id}` (platform keys with `partners:manage`,
  [REST API › Partners](../../reference/api.md#partners)); they are never deleted. `status` is read by
  authentication for every partner key and for every tenant and identity key of a partner's tenant
  ([Security § 4.2](security.md#42-verification), step 9), and by the `Deliver` consumer, which holds
  deliveries while the partner is `suspended` ([Webhooks › Delivering an attempt](webhooks.md#delivering-an-attempt)).
  `default_billing_mode` and `max_tenants` are read by `POST /v1/tenants` with a partner key, which writes
  the first to the new tenant's `billing_accounts.mode` and the key's `partner_id` to `tenants.partner_id`.
  `ramp_exempt` is read by the send-ramp evaluation and outbound policy step 18
  ([Cloud sign-up § 10.1](cloud-signup.md#101-new-workspace-send-ramp)). `tenants.partner_id` is read by
  the owner check of every partner-key request ([Security § 5.2](security.md#52-order-of-checks), step 4),
  by the `partner_id` filter of `GET /v1/tenants`, by the webhook fan-out to find a tenant's partner
  endpoints ([Webhooks › Endpoint resolution](webhooks.md#endpoint-resolution-and-filters)), and by the
  outbox dispatch, which copies it to `event_index.partner_id` (read by replay). `tenants.suspended_by` is
  written with `status` by `PATCH /v1/tenants/{tenant_id}` and read by the next status change (a partner
  key cannot lift `platform`). `tenants.policy_ceilings_json` is written when a platform key sets a
  lower-only policy field and read when a partner key or a workspace writer writes one;
  `tenants.partner_ceilings_json` is written when the tenant's partner key sets one and read when a
  workspace writer writes one; `tenants.policy_version` is written by every policy write and read by the
  next one's compare-and-set, and by the console's policy page
  ([Configuration › Who may change a field](../../reference/configuration.md#who-may-change-a-field),
  [Workspace policy](workspace-policy.md)).
  `api_keys.partner_id` is written by `POST /v1/keys` for `level: "partner"` and read by authentication
  (step 9). `webhook_endpoints.partner_id` is written by `POST /v1/webhooks` with a partner key and read
  by the fan-out, the replay selection and the owner check. `zone_claims` rows are written by the
  `nameservers` and `delegated_subdomain` onboarding, `pending` before `POST /zones` and `active` in the
  batch that inserts the domain row, read by the zone-permission check of every method and by the
  `delete_zone` step ([Identities and domains › Zone permission](identity-domains.md#zone-permission)), and
  deleted by `delete_zone`, when the zone expires, or by the cleanup of a failed add.
  `domain_onboarding` rows are written by `POST /v1/tenants/{tenant_id}/domains` (and by the monitor's
  onboarding of a created zone, through `domains.provider_objects_json` directly), read by the name-taken
  check, the unverified cap, the SES identity count and the hourly cleanup (`crons/domain_cleanup.rs`), and
  deleted by the domain insert batch or the cleanup. `domains.provider_objects_json` is written by that
  batch and by the monitor's onboarding steps, and read by every `domain_remove` step and by the J5
  failover step. `domains.ownership_verified_at` is also read by the unverified cap, the 14-day expiry and
  eviction. A policy write that drops a zone from `domains.cloudflare_zones` reads `domains` (`name`,
  `method`) to refuse while the tenant still has domains under it.
  `DELETE /v1/partners/{partner_id}` runs one D1 batch: it deletes the partner's `webhook_endpoints`
  (their deliveries cascade), revokes and deletes its `api_keys` and deletes its `idempotency_records`
  (`scope` = the partner ID), then sets `status = 'deleted'`, `name = ''` and `deleted_at`. Every statement
  of the batch carries the guard
  `AND NOT EXISTS (SELECT 1 FROM tenants WHERE partner_id = ?1 AND status <> 'erased')`, so while any of
  its tenants is not erased the batch changes nothing and the route answers `409 partner_has_tenants`; a
  tenant created concurrently is either seen by the guard or refused, because tenant creation requires the
  partner to be `active` in its own insert. `tenants.partner_id` keeps pointing at the deleted row
  ([Privacy § 6.10](privacy.md#610-partners)).
- **Console token hashes** (`invitations.token_hash`, `login_tokens.token_hash` and `code_hash`,
  `sessions.id_hash`, `pending_auth.id_hash`, `oauth_states.state_hash` and `cookie_hash`) use the current
  `link` key and record
  its kid in `key_kid`. A lookup computes the HMAC under each `link` key still inside its verify window,
  newest first. A session used while its `key_kid` is not current is re-hashed under the current key in
  the same `UPDATE` that moves `last_seen_at`; a session idle past its 7-day rolling lifetime is expired
  anyway, so the 7-day verify window of an old link key loses no live session.
- **SES ingestion ledger.** `ses_ingest` makes mail received through SES arrive exactly once per S3
  object and recipient, whichever path (SNS push or SQS backstop) delivers the notification first. A row
  becomes `done` when the message is committed, `dropped` for an unknown recipient, `held` while the
  recipient's tenant is suspended (at most 5 days, then `dropped`), and `lost` when the S3 object vanished
  before ingestion (alert `ses_object_lost`). The every-minute backstop cron re-sends the pointers of rows
  still `queued` after 15 minutes and of `held` rows whose tenant is active again. The S3 object is
  deleted once no row for its key is still `queued` or `held`
  ([Domains on any DNS host § 4.5–4.6](domain-connections.md#45-inbound-through-ses)).
- **Write volume.** D1 receives about four writes per inbound message: the event index, and webhook
  delivery rows per endpoint (plus one `ses_ingest` row per recipient for mail received through SES). The
  per-message mailbox writes go to the Durable Object. Retention jobs prune `webhook_deliveries` and
  `event_index` after the tenant's `policy.retention.events_days` (default 30; rows with
  `tenant_id IS NULL` after 30 days), and `idempotency_records` and `ses_ingest` after 30 days.
- **Console retention.** `oauth_states` rows expire 10 minutes after creation and are deleted 24 hours
  after expiry, as `login_tokens` are; so are `pending_auth` rows (5 or 10 minutes), and the
  `users.totp_pending_*` columns are cleared 24 hours after they expire. `waitlist` entries exist only once confirmed and are deleted 30
  days after invitation (Cloud sign-up § 6.1). `invitations` with status `expired` or `revoked` are
  deleted 30 days after `expires_at`; accepted ones stay with the workspace. Erasure of a person deletes
  their `oauth_identities` and any `waitlist` row and scrubs the address of their accepted invitations
  ([Privacy § 6.9](privacy.md#69-people-console-accounts)).
- **Billing events retention.** `billing_events` rows are deleted 400 days after `received_at` by the
  global retention job ([Privacy § 5.3](privacy.md#53-global-retention-job)).
- **Platform state.** `platform_state` holds one row per deployment-wide switch. `send_breaker` is
  written and deleted by the `*/15` cron's shared-domain breaker and read, through a 60-second isolate
  cache, by the send handler for outbound policy step 18
  ([Cloud sign-up § 10.3](cloud-signup.md#103-shared-domain-breaker)). It holds no personal data.
- **Erasure requests and exports.** `erasure_requests.deadline_at` is written at insert (`created_at` +
  24 hours, or the resumed request's value) and read by the JobRunner's retry limit, the
  `erasure_overdue` alert and the API object. `resumes_id` is written when a request of any scope resumes
  a failed request, when a tenant-scope request continues a `completed_with_holds` one, and by the
  `held_erasures` and identity hold continuations (at most one per request: `erasure_requests_resumes`); it is returned in the API object. `identity_id` and `target_id` are returned in the
  API object and read by `POST /v1/platform/erasure-requests/{erasure_id}/reapply`, with
  `counterparty_hash` ([Privacy § 11](privacy.md#11-what-remains-after-deletion)).
  `exports.identity_id` is written by `POST /v1/exports` and returned in the export object;
  `erasure_requests.completed_at` and `status` are also read by an export's `complete` step
  ([Privacy § 6.11](privacy.md#611-exports-and-copies-in-flight-i11)).
- **Parity tables and columns.** Each page that adds them names its writers and readers:
  `draft_index` ([Drafts § 9.2–9.3](drafts.md#92-d1-draft_index)), `tenants.events_do_id`
  ([Event feed § 2.1](event-feed.md#21-identity-and-lifecycle)), `agent_signups`, `agent_signup_refusals`,
  `tenants.provisional_until`, `pending_auth.agent_signup_id` and the `agent_signup` switch
  ([Agent self-sign-up § 15](agent-signup.md#15-storage)), `api_keys.mcp_tools_json`
  ([SDKs, skills and agent tooling § 5.3](sdk-and-skills.md#53-per-key-tool-allow-list)),
  `identities.message_hours` ([Parity additions § 7.2](parity-additions.md#72-how-the-mailbox-learns-it)) and the
  `webhook_endpoints` header columns ([Parity additions § 8.3](parity-additions.md#83-delivery-and-storage)).
  Tenant erasure deletes `draft_index` and `agent_signups` rows in `delete_d1_rows`, `agent_signups` before
  `api_keys` and `identities` ([Privacy § 6.6](privacy.md#66-tenant-scope)); `agent_signup_refusals` is never
  deleted by erasure, because it records an objection to contact, as `suppressions` does.

- **Phone numbers.** `phone_numbers` is both the routing directory (one live row per E.164, enforced by
  `phone_numbers_live`) and the history of ownership periods. Requests, numbers, registrations and port-outs
  move only under the execution lease of [Phone numbers § 5.5](phone-numbers.md#55-execution-leases-and-attempts);
  `customer_reference` is unique so a recovered order can be attributed to exactly one attempt. The standing
  budget's month count and the phone spend holds live in `TenantQuota` (`phone_budget:t:*`, `phone_spend:*`),
  not in D1.
- **Vault.** Every `vault_*` table is written by the vault handlers and jobs named in [Vault](vault.md)
  (§ 3–§ 16) and read by the same routes, the consumer routes, the every-minute vault cron, the daily vault
  sweep, the global retention job's `vault` step and the erasure steps.

## 2. `IdentityMailbox` Durable Object (SQLite)

One object per identity. Every write path runs inside `transaction_sync` (or the `workers-rs`
equivalent) so that the message, the index and the outbox commit together.

```sql
-- mailbox schema v1
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys: schema_version, tenant_id, identity_id, created_at, erased ('0'|'1'),
--       event_seq, fts_analyzer_version, embed_model,
--       size_bytes, size_checked_at  pragma page_count * page_size, refreshed at most hourly after a write
--                       and by the daily maintenance; the check reads the previous value and reports
--                       mailbox_size when it crosses 70% of 10 GB (Mailbox notes)
--       claim:{msg}     transport claim {token, claimed_at} (Outbound › The outbound consumer)
--       backoff:{msg}   quota/rate back-off {n, first_at} while the message stays queued
--       dispatch:{msg}  time the send pointer was (re)queued; read by the dispatch alarm
--       wait:{domain}   time until which a wait for that sender domain counts (RegisterWait: now + timeout
--                       + 10 s, refreshed every 10 s; Inbound › The wait handler, E4; read by the
--                       unsolicited-code check, E5)
--       is_system       '1' in the system identity's mailbox (written by Init): redacted bodies, no index,
--                       no tenant fan-out (Inbound › The system identity's mailbox, A15)
--       parse:{msg}     parse attempts of an inbound pointer (NoteParseAttempt); deleted by ingest, or
--                       by the daily maintenance after a day (Inbound › consumer step 4)
--       compose:{msg}   time an outbound sent copy was about to be written; deleted by the accept
--                       transaction or the refusal path; a stale one names an orphan (Inbound › Orphan objects)
--       orphan_sweep_cursor  the last R2 key the rolling orphan sweep listed (I9)
--       outbox_backoff  consecutive failed outbox dispatches; the retry delay is 30 s doubled per failure,
--                       at most 5 minutes; cleared by a successful dispatch (Webhooks › Dispatching)
--       retention_message_hours, retention_version  the identity's retention.message_hours and the
--                       identities.updated_at of the last SetRetention applied (Parity additions § 7.2)
--       expiry_backoff  consecutive failed expiry slices; 30 s doubled, at most 5 minutes
--       expiry:{YYYY-MM-DD}  that UTC day's expiry counts, flushed to the audit log by maintenance
--       draft_index_backoff  consecutive failed draft_index writes to D1 (Drafts § 9.3)
--       draft_att:{attachment_id}  an inline draft attachment written to R2 and not yet committed (Drafts § 7)
--       screen_freeze_seen  '1' when the screen alarm ran during PM_FREEZE; cleared by whatever first applies
--                       the extension after it (Screening § 6.1)
--       events_do_id    the tenant's TenantEvents ID, read once from D1 by the log_erase purpose
--       log_erase_backoff  consecutive failed log_erase calls; 30 s doubled, at most 5 minutes (Event feed § 11.3)
--       screen_hour     {"h": "YYYY-MM-DDTHH", "n": hold-mode verdicts, "u": of them unavailable, "r": 0|1}
--                       for the screening_unavailable condition (Screening § 22); replaced when the UTC hour
--                       changes; "r" = 1 once the condition was reported in that hour
--       alarm:{purpose} pending wake-ups: outbox, claim, dispatch, maintenance (Design § 4); drafts (scheduled
--                       sends, claims and approval expiry) and draft_index (Drafts § 6.2, § 9.3); screen (the
--                       earliest deadline_at of a pending screenings row, Screening § 6.1); uploads (the
--                       earliest upload expiry, tombstone purge or object-delete retry, Uploads § 5.3); expiry
--                       (per-identity retention, Parity additions § 7.4); log_erase (erased_ids rows not yet
--                       sent to the event log, Event feed § 11.3). Thread locks expire lazily and
--                       reconciliation is event-driven, so neither has an alarm.
-- parser_version is a column of messages (and a core constant), not a meta key.

CREATE TABLE IF NOT EXISTS threads (
  seq                INTEGER PRIMARY KEY AUTOINCREMENT,    -- per mailbox; used in thread tokens; never reused,
                                                           -- even after the newest thread is erased (C1)
  id                 TEXT NOT NULL UNIQUE,                 -- thr_
  -- The summary columns below count VISIBLE messages only (not quarantined, screening, hidden or throttled),
  -- recomputed by Threading § 3.4; message_count = 0 means the thread is never listed
  subject            TEXT NOT NULL,                        -- normalised subject of the first visible message ('' none yet)
  first_at           INTEGER NOT NULL,
  last_at            INTEGER NOT NULL,                     -- latest visible message, else first_at
  last_inbound_at    INTEGER,                              -- latest visible inbound message
  last_outbound_at   INTEGER,
  message_count      INTEGER NOT NULL DEFAULT 0,           -- visible messages
  unread_count       INTEGER NOT NULL DEFAULT 0,           -- visible inbound messages with read = 0
  participants_json  TEXT NOT NULL DEFAULT '[]',           -- [{address,name}] of visible messages, capped at 50
  reply_from_address TEXT,                                 -- address the counterparty last wrote to
  fallback_pinned    INTEGER NOT NULL DEFAULT 0,           -- stays on platform address until quiet
  hold_json          TEXT,                                 -- legal hold {reason, until, set_by, set_at}
  lock_owner         TEXT,                                 -- send lock (FR-OUT-9)
  lock_until         INTEGER,
  archived           INTEGER NOT NULL DEFAULT 0,
  category           TEXT,                                 -- from the latest triaged inbound message
  needs_reply        REAL,
  urgency            INTEGER
);
CREATE INDEX IF NOT EXISTS threads_last ON threads(last_at DESC);

CREATE TABLE IF NOT EXISTS messages (
  rowid               INTEGER PRIMARY KEY,                 -- FTS rowid
  id                  TEXT NOT NULL UNIQUE,                -- msg_
  thread_seq          INTEGER NOT NULL REFERENCES threads(seq),
  direction           TEXT NOT NULL CHECK (direction IN ('inbound','outbound')),
  status              TEXT NOT NULL CHECK (status IN (
                        'received','quarantined','screening','throttled','hidden',         -- inbound
                        'queued','submitted','delivered','deferred','bounced','complained',
                        'rejected','failed','uncertain','suppressed','canceled')),         -- outbound
  rfc_message_id      TEXT,                                -- normalised, without angle brackets
  message_id_synthetic INTEGER NOT NULL DEFAULT 0,         -- B3: hash-derived when missing
  provider            TEXT                                 -- cloudflare | ses | smtp | simulator | loopback
                      CHECK (provider IN ('cloudflare','ses','smtp','simulator','loopback')),
  provider_message_id TEXT,
  in_reply_to         TEXT,
  references_json     TEXT NOT NULL DEFAULT '[]',
  raw_sha256          TEXT,
  raw_r2_key          TEXT,
  raw_size            INTEGER,
  from_address        TEXT,
  from_name           TEXT,
  sender_domain       TEXT,                                -- organisational domain of From
  reply_to_json       TEXT NOT NULL DEFAULT '[]',
  to_json             TEXT NOT NULL DEFAULT '[]',
  cc_json             TEXT NOT NULL DEFAULT '[]',
  bcc_json            TEXT NOT NULL DEFAULT '[]',          -- outbound only
  delivered_to        TEXT,                                -- inbound envelope recipient
  is_bcc              INTEGER NOT NULL DEFAULT 0,          -- A10
  is_primary_recipient INTEGER NOT NULL DEFAULT 1,         -- A9: 1 on exactly one copy per tenant (Inbound)
  subject             TEXT,
  text                TEXT,                                -- full plain text (derived if HTML-only)
  html_sanitized      TEXT,
  extracted_text      TEXT,                                -- new content: quotes and signature removed
  snippet             TEXT,                                -- ≤ 240 chars of extracted_text
  sent_at             INTEGER,                             -- Date header, or submit time
  received_at         INTEGER NOT NULL,                    -- our clock
  received_evented_at INTEGER,                             -- when message.received was first appended for this
                                                           -- message; NULL until then. Written by ingest (status
                                                           -- received), the screening commit (clear, or
                                                           -- unavailable delivered) and release; read by release,
                                                           -- which appends message.received only while it is
                                                           -- NULL (Screening § 10)
  kind                TEXT NOT NULL,                       -- inbound: normal|automated|dsn|list|calendar|mdn
                                                           -- outbound: transactional|marketing|auto_reply
  automated_json      TEXT,                                -- classification evidence
  auth_json           TEXT,                                -- spf, dkim[], dmarc, arc, authserv; spf.aligned;
                                                           -- arc.sealer, arc.sealer_trusted, arc.aar, arc.rescue
                                                           -- (Parity additions § 5.2, § 6.3)
  verdict             TEXT CHECK (verdict IN ('pass','arc_rescued','fail','softfail','none','unaligned',
                                              'unverified')),
  spam_score          REAL,
  known_sender        INTEGER,
  quarantine_reason   TEXT                                 -- NULL unless status is quarantined or hidden
                      CHECK (quarantine_reason IN ('auth_failed','auth_unverified','spam','risky_attachment',
                                                   'blocked_sender','otp_unsolicited','account_unapproved',
                                                   'steering_suspected','screening_unavailable','vault_code')),  -- blocked_sender:
                                                           -- a receive-block entry only (D7); the last two:
                                                           -- Screening § 4.3, § 7; vault_code: Vault § 9.2
  flags_json          TEXT NOT NULL DEFAULT '[]',          -- parse_degraded, message_id_conflict, encrypted,
                                                           -- hidden_text, sent_via_fallback, reprocessed, bcc,
                                                           -- thread_join_unverified, reconciled, loopback,
                                                           -- body_truncated, display_name_spoof,
                                                           -- lookalike_domain, reply_to_mismatch,
                                                           -- shared_domain_sender, sender_suppressed,
                                                           -- body_redacted, dsn_untrusted, steering_suspected,
                                                           -- attachments_unscreened. The
                                                           -- API returns the trust ones (hidden_text,
                                                           -- display_name_spoof, lookalike_domain, reply_to_mismatch,
                                                           -- thread_join_unverified, shared_domain_sender,
                                                           -- steering_suspected, attachments_unscreened) in
                                                           -- trust.flags, the rest in flags. Every row stays within
                                                           -- the 1,900,000-byte budget of Inbound › Storage caps
  read                INTEGER NOT NULL DEFAULT 0,
  triage_status       TEXT CHECK (triage_status IN ('pending','done','skipped','failed')),
  triage_json         TEXT,
  operation           TEXT                                 -- send | reply | reply_all | forward; NULL for inbound
                      CHECK (operation IN ('send','reply','reply_all','forward')),
  draft_id            TEXT,                                -- the draft an outbound message was sent from (Drafts § 5.4)
  parser_version      INTEGER,
  metadata_json       TEXT NOT NULL DEFAULT '{}',
  calendar_json       TEXT,                                -- the calendar object (Calendar invitations §2.9),
                                                           -- at most 262,144 bytes; NULL without a calendar part
  calendar_uid_hash   TEXT                                 -- hex SHA-256 of the primary UID's bytes; NULL unless
                                                           -- object_status = 'parsed'
);
CREATE INDEX IF NOT EXISTS messages_thread   ON messages(thread_seq, received_at);
CREATE INDEX IF NOT EXISTS messages_rfcid    ON messages(rfc_message_id);
CREATE INDEX IF NOT EXISTS messages_provider ON messages(provider_message_id);
CREATE INDEX IF NOT EXISTS messages_hash     ON messages(raw_sha256);
CREATE INDEX IF NOT EXISTS messages_time     ON messages(received_at DESC);
CREATE INDEX IF NOT EXISTS messages_from     ON messages(from_address);
CREATE INDEX IF NOT EXISTS messages_status   ON messages(direction, status);
CREATE INDEX IF NOT EXISTS messages_calendar_uid ON messages(calendar_uid_hash)
  WHERE calendar_uid_hash IS NOT NULL;

CREATE TABLE IF NOT EXISTS deliveries (                    -- per-recipient outbound status
  message_rowid   INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  address         TEXT NOT NULL,
  field           TEXT NOT NULL CHECK (field IN ('to','cc','bcc')),
  status          TEXT NOT NULL CHECK (status IN ('queued','suppressed','submitted','delivered','deferred',
                                                  'bounced','complained','rejected','failed','uncertain')),
  smtp_code       TEXT,
  enhanced_code   TEXT,
  smtp_response   TEXT,                                    -- trimmed to 512 chars
  bounce_type     TEXT CHECK (bounce_type IN ('hard','soft')),
  provider_event_ids_json TEXT NOT NULL DEFAULT '[]',      -- dedupe of provider events
  first_reply     INTEGER NOT NULL DEFAULT 0 CHECK (first_reply IN (0,1)),  -- 1: a reply to someone who
                                                           -- wrote in and was never written to; its outcome
                                                           -- counts towards no pause (Outbound › First replies)
  updated_at      INTEGER NOT NULL,
  PRIMARY KEY (message_rowid, address)
);

CREATE TABLE IF NOT EXISTS attachments (
  id            TEXT PRIMARY KEY,                          -- att_
  message_rowid INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  filename      TEXT,                                      -- sanitised: no path, ≤ 255 bytes
  content_type  TEXT NOT NULL,                             -- as declared
  sniffed_type  TEXT,                                      -- from magic bytes; wins on conflict (B10)
  size          INTEGER NOT NULL,
  sha256        TEXT NOT NULL,
  disposition   TEXT CHECK (disposition IN ('attachment','inline')),
  content_id    TEXT,
  r2_key        TEXT NOT NULL,
  text_status   TEXT NOT NULL CHECK (text_status IN ('pending','ready','unavailable','skipped')),
  text_r2_key   TEXT,
  text_pages    INTEGER,
  risk          TEXT CHECK (risk IN ('executable','macro','encrypted_archive','archive_bomb',
                                     'type_mismatch','encrypted_document')),
  scan_status   TEXT NOT NULL DEFAULT 'skipped' CHECK (scan_status IN ('skipped','pending','clean','infected','error')),
  upload_id     TEXT                                       -- upl_ a sent attachment was copied from; NULL for inline
                                                           -- base64 and inbound. Not a foreign key: the upload may be
                                                           -- gone (Uploads § 5.1)
);
CREATE INDEX IF NOT EXISTS attachments_message ON attachments(message_rowid);

CREATE TABLE IF NOT EXISTS labels (
  message_rowid INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  label         TEXT NOT NULL,                             -- ^[a-z0-9][a-z0-9_:-]{0,63}$
  PRIMARY KEY (message_rowid, label)
);
CREATE INDEX IF NOT EXISTS labels_label ON labels(label);

-- Keyword index. Contentless (text lives in messages); snippets are built in Rust.
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
  subject, participants, body_new, body_full, attachments, refs,
  content = '', contentless_delete = 1,
  tokenize = 'unicode61 remove_diacritics 2'
);
-- Fuzzy fallback over short fields only (bounded size).
CREATE VIRTUAL TABLE IF NOT EXISTS fts_tri USING fts5(
  subject, participants, refs,
  content = '', contentless_delete = 1,
  tokenize = 'trigram'
);

CREATE TABLE IF NOT EXISTS refs (
  message_rowid INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  kind          TEXT NOT NULL,                             -- uk_plate, pcn, invoice, order, amount, phone,
                                                           -- email, domain, date, custom:<name> (custom:booking)
  value         TEXT NOT NULL,                             -- normalised: AB12CDE, +447700900123, GBP:412.80
  source        TEXT NOT NULL,                             -- subject | body | att:<att_id>:<page>
  PRIMARY KEY (message_rowid, kind, value, source)
);
CREATE INDEX IF NOT EXISTS refs_kind_value ON refs(kind, value);
CREATE INDEX IF NOT EXISTS refs_value      ON refs(value);

CREATE TABLE IF NOT EXISTS contacts (
  address         TEXT PRIMARY KEY,
  name            TEXT,
  domain          TEXT NOT NULL,
  first_seen_at   INTEGER NOT NULL,
  last_seen_at    INTEGER NOT NULL,
  inbound_count   INTEGER NOT NULL DEFAULT 0,
  outbound_count  INTEGER NOT NULL DEFAULT 0,
  last_thread_seq INTEGER
);
CREATE INDEX IF NOT EXISTS contacts_domain ON contacts(domain);

CREATE TABLE IF NOT EXISTS idempotency (
  key_hash      TEXT PRIMARY KEY,                          -- hex SHA-256 of the Idempotency-Key header; a draft
                                                           -- send's internal key is namespaced (Drafts § 5.1)
  fingerprint   TEXT NOT NULL,                             -- sha256(operation, target, canonical body)
  operation     TEXT NOT NULL,                             -- send, reply, reply_all, forward; and draft_create,
                                                           -- draft_send, draft_approve, draft_reject (Drafts § 9.1);
                                                           -- batch_update, written in the transaction of the change,
                                                           -- response_json: IDs and counts (Parity additions § 2.4)
  message_id    TEXT,
  draft_id      TEXT,                                      -- the draft a draft route's record, or a draft send's
                                                           -- ledger row, belongs to; erasure deletes by it (Drafts § 13.2)
  response_json TEXT NOT NULL,                             -- the 202 Message with extracted_text, text and html
                                                           -- set to null; a replay fills them from the row
                                                           -- (Outbound › Reservation), so it stays a few KB. After
                                                           -- expiry (Parity additions § 7.6) only
                                                           -- {expired_at, message_id}, with fingerprint replaced by
                                                           -- its HMAC under PM_HASH_KEY
  created_at    INTEGER NOT NULL,
  expires_at    INTEGER NOT NULL                           -- created_at + 30 days
);
CREATE INDEX IF NOT EXISTS idempotency_draft ON idempotency(draft_id) WHERE draft_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS delivery_stubs (                -- content-free delivery state of an outbound message
  provider_message_id TEXT NOT NULL,                       -- that per-identity expiry deleted, so late bounces and
  address_hash        TEXT NOT NULL,                       -- complaints still suppress and count (Parity additions
  message_id          TEXT NOT NULL,                       -- § 7.13). address_hash: hex HMAC-SHA256(PM_HASH_KEY,
  recipient_domain    TEXT NOT NULL,                       -- normalised recipient); recipient_domain for
  domain_id           TEXT,                                -- metric_counts; domain_id and first_reply for
  status              TEXT NOT NULL,                       -- RecordOutcome; status and provider_event_ids_json as on
  first_reply         INTEGER NOT NULL,                    -- deliveries. Written by EraseRows (mode Expiry), read
  provider_event_ids_json TEXT NOT NULL DEFAULT '[]',      -- by ApplyDeliveryEvent; deleted at expires_at by the
  sent_at             INTEGER NOT NULL,                    -- daily maintenance, by counterparty erasure of the
  expires_at          INTEGER NOT NULL,                    -- address, and with the mailbox. expires_at = sent_at
  PRIMARY KEY (provider_message_id, address_hash)          -- + 30 days
);

CREATE TABLE IF NOT EXISTS erased_ids (                    -- message IDs an erasure deleted (I10): writers that
  message_id TEXT PRIMARY KEY,                             -- finish after the erasure see them and delete what they
  erased_at  INTEGER NOT NULL,                             -- wrote; ingest refuses them. Rows older than 7 days are
                                                           -- deleted by the daily maintenance, once log_erased_at is set
  log_erased_at INTEGER                                    -- NULL until the alarm purpose log_erase has sent the ID
);                                                         -- to the tenant's event log (Event feed § 11.3)

CREATE TABLE IF NOT EXISTS outbox (                        -- transactional event outbox
  seq          INTEGER PRIMARY KEY,                        -- per-identity event sequence
  event_id     TEXT NOT NULL UNIQUE,                       -- evt_
  type         TEXT NOT NULL,
  payload_json TEXT NOT NULL,                              -- full event envelope
  occurred_at  INTEGER NOT NULL,
  dispatched_at INTEGER                                    -- set once queued to pm-webhooks
);
CREATE INDEX IF NOT EXISTS outbox_pending ON outbox(dispatched_at) WHERE dispatched_at IS NULL;

CREATE TABLE IF NOT EXISTS chunks (                        -- semantic index bookkeeping
  vector_id     TEXT PRIMARY KEY,                          -- {msg_id}:{n} or {msg_id}:a{k}:{n} (≤ 64 bytes)
  message_rowid INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  attachment_id TEXT,
  ordinal       INTEGER NOT NULL,
  page          INTEGER,
  char_start    INTEGER NOT NULL,
  char_end      INTEGER NOT NULL,
  model         TEXT NOT NULL,                             -- e.g. bge-m3@1
  status        TEXT NOT NULL CHECK (status IN ('pending','embedded','failed','deleting')),
  updated_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS chunks_status ON chunks(status);
CREATE INDEX IF NOT EXISTS chunks_message ON chunks(message_rowid);   -- coverage, read-back, deletes

CREATE TABLE IF NOT EXISTS verifications (                 -- codes and links for wait / sign-ups
  message_rowid INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  kind          TEXT NOT NULL CHECK (kind IN ('code','link')),
  value         TEXT NOT NULL,
  sender_domain TEXT NOT NULL,
  expires_at    INTEGER NOT NULL,                          -- received + 24 h; purged after
  consumed_at   INTEGER,                                   -- first released by wait; purged 1 h later
  account_id    TEXT                                       -- sac_ of the approved service-ledger entry that
                                                           -- matched (rule 4a); NULL when the tenant does not
                                                           -- require approval. Read by wait before a release
  vault_item_id     TEXT,                                  -- set by quarantine rule 4b (Vault § 9.2)
  vault_use_id      TEXT,                                  -- set by VaultTakeCode
  vault_consumed_at INTEGER                                -- set by VaultTakeCode; a consumed code is never released
);

-- Drafts, approvals and scheduled send (Drafts § 9.1)
CREATE TABLE IF NOT EXISTS drafts (
  rowid              INTEGER PRIMARY KEY,
  id                 TEXT NOT NULL UNIQUE,                 -- drf_
  client_id          TEXT,                                 -- caller's idempotent-create key (Drafts § 4.1)
  client_fingerprint TEXT,                                 -- sha256 of the canonical create body
  draft_kind         TEXT NOT NULL CHECK (draft_kind IN ('new','reply','reply_all','forward')),
  kind               TEXT NOT NULL CHECK (kind IN ('transactional','marketing','auto_reply')),
  status             TEXT NOT NULL CHECK (status IN ('editing','pending_approval','approved','scheduled',
                                                     'sending','sent','failed','rejected','canceled')),
  status_at          INTEGER NOT NULL,                     -- when it entered this status
  revision           INTEGER NOT NULL DEFAULT 1,
  index_version      INTEGER NOT NULL DEFAULT 1,           -- +1 on every write; guards the D1 index (Drafts § 9.3)
  digest             TEXT,                                 -- hex sha256 of the current content (Drafts § 2.4); NULL once cleared
  source_message_id  TEXT,                                 -- in_reply_to or forward_of (msg_); NULL for new
  thread_seq         INTEGER,                              -- the source's thread, or thread_id of a new draft;
                                                           -- no foreign key: erasure deletes the draft first (§ 13.2)
  from_address       TEXT,
  to_json            TEXT NOT NULL DEFAULT '[]',
  cc_json            TEXT NOT NULL DEFAULT '[]',
  bcc_json           TEXT NOT NULL DEFAULT '[]',
  resolved_json      TEXT,                                 -- reply kinds: {to, cc} as resolved for the last digest;
                                                           -- rewritten with a new revision when it changes (Drafts § 2.4)
  subject            TEXT,
  text               TEXT,                                 -- ≤ 524 288 bytes
  html               TEXT,                                 -- ≤ 1 048 576 bytes, as written
  headers_json       TEXT NOT NULL DEFAULT '{}',
  attachments_json   TEXT NOT NULL DEFAULT '[]',           -- [{id, filename, content_type, disposition,
                                                           --   content_id, size, sha256, sniffed_type, source, r2_key,
                                                           --   upload_id?}]
  include_attachments INTEGER,                             -- forward only
  calendar_json      TEXT,                                 -- new and reply drafts: the calendar field of the send
                                                           -- routes ({ics, attach_copy}, Calendar invitations § 5.1),
                                                           -- part of the digest; NULL otherwise
  unsubscribe_json   TEXT,
  consent_json       TEXT,
  labels_json        TEXT NOT NULL DEFAULT '[]',
  metadata_json      TEXT NOT NULL DEFAULT '{}',
  send_at            INTEGER,
  next_attempt_at    INTEGER,                              -- a scheduled fire waiting after a transient condition
  approval_json      TEXT NOT NULL DEFAULT '{"state":"none"}',  -- Drafts § 2.1 approval
  send_json          TEXT,                                 -- Drafts § 2.1 send request, with the authority
  claim_json         TEXT,                                 -- {token, claimed_at, prior_status, origin, attempt}
  rejection_json     TEXT,
  failure_json       TEXT,
  message_id         TEXT,                                 -- msg_ once sent
  sent_at            INTEGER,
  created_by_key_id  TEXT,
  updated_by_key_id  TEXT,
  content_by_key_id  TEXT,                                 -- the key that last changed the content: self-approval
                                                           -- and the writer rule read it (Drafts § 2.1)
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS drafts_client  ON drafts(client_id) WHERE client_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_due     ON drafts(status, send_at);
CREATE INDEX IF NOT EXISTS drafts_source  ON drafts(source_message_id) WHERE source_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_thread  ON drafts(thread_seq) WHERE thread_seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_message ON drafts(message_id) WHERE message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS drafts_created ON drafts(created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS drafts_updated ON drafts(status, updated_at);

CREATE TABLE IF NOT EXISTS draft_sync (                     -- index rows waiting to be written to D1 (Drafts § 9.3)
  draft_id  TEXT PRIMARY KEY,
  version   INTEGER NOT NULL,                              -- drafts.index_version it describes
  op        TEXT NOT NULL CHECK (op IN ('upsert','delete')),
  row_json  TEXT,                                          -- the draft_index row for an upsert: IDs, status, kind,
                                                           -- counts, times, key IDs; never content
  queued_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS screenings (           -- one row per screened inbound message (Screening § 3, § 14)
  message_rowid  INTEGER PRIMARY KEY REFERENCES messages(rowid) ON DELETE CASCADE,
  mode           TEXT NOT NULL CHECK (mode IN ('monitor','hold')),
  status         TEXT NOT NULL CHECK (status IN ('pending','clear','suspect','unavailable')),
  reason         TEXT CHECK (reason IN ('rules','model','late_signal','model_clear','nothing_to_screen',
                                        'deadline_exceeded','model_unavailable','invalid_output',
                                        'ai_rate_limited','model_cap','test_forced')),   -- NULL while pending
  signals_json   TEXT NOT NULL DEFAULT '[]',      -- [{source, kind, field, severity}], at most 32; no text
  model          TEXT,                            -- model ID, 'rules', or NULL
  on_unavailable TEXT NOT NULL CHECK (on_unavailable IN ('quarantine','deliver')),   -- fixed at ingest
  account_id     TEXT,                            -- sac_ matched by rule 4a at ingest; read by the clear
                                                  -- commit for the verifications row
  outcome        TEXT CHECK (outcome IN ('received','quarantined','unchanged')),    -- NULL while pending
  started_at     INTEGER NOT NULL,                -- ingest commit
  deadline_at    INTEGER NOT NULL,                -- started_at + deadline_seconds × 1000
  completed_at   INTEGER,
  late           INTEGER NOT NULL DEFAULT 0,      -- 1 once a late signal changed the verdict (Screening § 8)
  released_by_key_id  TEXT,                       -- set by release (Screening § 10)
  released_by_user_id TEXT,
  released_at    INTEGER
);
CREATE INDEX IF NOT EXISTS screenings_pending ON screenings(deadline_at) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS screenings_started ON screenings(started_at);

CREATE TABLE IF NOT EXISTS screening_feedback (   -- Screening § 11; at most 20 rows per message
  id               TEXT PRIMARY KEY,              -- sfb_
  message_rowid    INTEGER NOT NULL REFERENCES messages(rowid) ON DELETE CASCADE,
  verdict          TEXT NOT NULL CHECK (verdict IN ('correct','false_positive','missed_threat',
                                                    'legitimate_instruction','spam')),
  screening_status TEXT NOT NULL CHECK (screening_status IN ('clear','suspect','unavailable')),
  notes            TEXT,                          -- ≤ 500 characters, the caller's text; never mail content
  actor_key_id     TEXT,                          -- one of the two is set
  actor_user_id    TEXT,
  created_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS screening_feedback_message ON screening_feedback(message_rowid, created_at);

CREATE TABLE IF NOT EXISTS uploads (                       -- files an agent staged to send (Uploads § 5.1)
  id            TEXT PRIMARY KEY,                          -- upl_
  filename      TEXT NOT NULL,                             -- sanitised: no path, ≤ 255 bytes
  content_type  TEXT NOT NULL,                             -- as declared, ≤ 127 bytes
  size          INTEGER NOT NULL CHECK (size BETWEEN 1 AND 26214400),
  sha256        TEXT NOT NULL,                             -- lower-case hex, 64 characters
  status        TEXT NOT NULL CHECK (status IN ('pending','uploading','ready','rejected','expired')),
  status_reason TEXT CHECK (status_reason IN ('risk','infected','expired','too_many_attempts','object_missing')),
  sniffed_type  TEXT,
  risk          TEXT CHECK (risk IN ('executable','macro','encrypted_archive','archive_bomb',
                                     'type_mismatch','encrypted_document')),
  scan_status   TEXT NOT NULL DEFAULT 'skipped' CHECK (scan_status IN ('skipped','clean','infected','error')),
  r2_key        TEXT NOT NULL,                             -- t/{ten}/i/{idn}/up/{upl}
  object_state  TEXT NOT NULL DEFAULT 'absent' CHECK (object_state IN ('absent','present','deleting')),
  put_token     TEXT,                                      -- the PUT holding the claim (status uploading)
  lease_until   INTEGER,                                   -- the claim lapses after this time
  failed_puts   INTEGER NOT NULL DEFAULT 0,                -- counted failures; the 10th expires the upload
  use_count     INTEGER NOT NULL DEFAULT 0,
  last_used_at  INTEGER,
  created_at    INTEGER NOT NULL,
  expires_at    INTEGER NOT NULL,
  ended_at      INTEGER                                    -- when it became rejected or expired
);
CREATE INDEX IF NOT EXISTS uploads_status_expiry ON uploads(status, expires_at);
CREATE INDEX IF NOT EXISTS uploads_created ON uploads(created_at, id);

CREATE TABLE IF NOT EXISTS metric_counts (                 -- event counts per hour and day (Deliverability
                                                           -- metrics § 4.2); no personal data
  bucket TEXT NOT NULL,                  -- 'h:YYYY-MM-DDTHH' (UTC hour) or 'd:YYYY-MM-DD' (UTC day)
  type   TEXT NOT NULL CHECK (type IN ('received','quarantined','sent','delivered','deferred','bounced',
                                       'complained','rejected','failed','uncertain','suppressed','canceled')),
  domain TEXT NOT NULL DEFAULT '',       -- lower-case A-label DNS name; '' when none
  value  INTEGER NOT NULL,
  PRIMARY KEY (bucket, type, domain)
) WITHOUT ROWID;                         -- h: rows deleted after 192 hours, d: rows after 92 days (maintenance)

CREATE TABLE IF NOT EXISTS rate_windows (                  -- inbound volume caps (D5, D13) and token
                                                           -- brute-force limits (D10); keys in Inbound › Inbound
                                                           -- volume caps and Threading § 2.5
  sender      TEXT NOT NULL,
  window_start INTEGER NOT NULL,
  count       INTEGER NOT NULL,
  PRIMARY KEY (sender, window_start)
);
```

```sql
-- crates/worker/src/mailbox/schema/00NN_sms.sql (M39)
CREATE TABLE IF NOT EXISTS sms_conversations (
  id                TEXT PRIMARY KEY,                      -- smc_
  number_id         TEXT NOT NULL,                         -- pnm_ of this identity
  counterparty      TEXT NOT NULL,                         -- E.164, short code or alphanumeric sender
  counterparty_hash TEXT NOT NULL,
  last_message_at   INTEGER NOT NULL,
  created_at        INTEGER NOT NULL,
  UNIQUE (number_id, counterparty)
);

CREATE TABLE IF NOT EXISTS sms_messages (
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
CREATE INDEX IF NOT EXISTS sms_messages_conversation ON sms_messages(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS sms_messages_status ON sms_messages(status, created_at);

CREATE TABLE IF NOT EXISTS sms_media (
  sms_id       TEXT NOT NULL REFERENCES sms_messages(id),
  n            INTEGER NOT NULL,
  content_type TEXT,
  size         INTEGER,
  status       TEXT NOT NULL CHECK (status IN ('pending','stored','unavailable','risky')),
  r2_key       TEXT,
  PRIMARY KEY (sms_id, n)
);

CREATE TABLE IF NOT EXISTS sms_verifications (
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

CREATE TABLE IF NOT EXISTS sms_waits (
  id          TEXT PRIMARY KEY,
  number_id   TEXT NOT NULL,
  from_party  TEXT,
  account_id  TEXT,
  expires_at  INTEGER NOT NULL
);

-- crates/worker/src/mailbox/schema/00NN_calls.sql (M40)
CREATE TABLE IF NOT EXISTS calls (
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
CREATE INDEX IF NOT EXISTS calls_number ON calls(number_id, created_at);
```

The mailbox `idempotency` table gains `sms_id TEXT` (nullable), the SMS a send key produced.

### Mailbox notes

- **Erasure** deletes the message row (cascading to deliveries, attachments, labels, refs, chunks,
  verifications, screenings and screening_feedback). It also deletes the FTS rows (`DELETE FROM fts WHERE rowid = ?`), and the R2 objects
  and vectors listed before the delete, and inserts each message ID into `erased_ids` in the same
  transaction, so an index job or post-commit step that finishes later deletes what it wrote
  ([Search § 6.7](search.md#67-deletion-on-erasure), [I10](../edge-cases.md)). An identity-scope erasure
  ends with `delete_all()` on the object. The drafts an erasure covers, with their `draft_sync` and
  `idempotency` rows, and the uploads its messages were copied from, are deleted in the same transaction
  ([Drafts § 13.2](drafts.md#132-erasure), [Uploads § 9](uploads.md#9-privacy)). See [Privacy and erasure](privacy.md).
- **Parity additions are in schema v1.** `drafts`, `draft_sync`, `screenings`, `screening_feedback`,
  `uploads`, `metric_counts`, `delivery_stubs`, `messages.draft_id`, `received_evented_at`, `calendar_json` and
  `calendar_uid_hash`, `attachments.upload_id`, `idempotency.draft_id` and `erased_ids.log_erased_at` are part
  of mailbox schema v1,
  because no mailbox has been built from these documents yet. A deployment whose mailboxes already run a v1
  without them needs a schema v2 migration, which rebuilds `messages` to widen its `CHECK` constraints and is
  designed with its own ADR before it is taken ([Screening § 14](screening.md#14-storage)).
- **Limits.** Durable Object SQLite allows 2 MB per string, BLOB or row, 100 bound parameters per query,
  100 KB per statement and 100 columns per table (read 2026-10-10). Every message row has a 1,900,000-byte
  budget ([Inbound › Storage caps](inbound.md#storage-caps)), and every list of values is bound as one
  JSON array expanded with `json_each`, so no statement binds more than 100 parameters. The widest tables,
  `messages` (49 columns) and `drafts` (43 columns), are under the limit.
- **Size watch.** After a write transaction, when `meta.size_checked_at` is more than an hour old, and in
  the daily maintenance, the mailbox computes `pragma page_count * page_size`, writes `meta.size_bytes`
  and `size_checked_at`, and writes one `mailbox_size_bytes` point. When the previous `size_bytes` was at
  or under 70% of 10 GB (7,516,192,768 bytes) and the new one is over it, it reports the `mailbox_size`
  condition ([Observability › Alerts](observability.md#5-alerts)). A mailbox grows only on writes, so an
  idle mailbox needs no hourly wake-up. Raw MIME and attachments are in R2, so mailboxes grow slowly.
- **Fallback if `contentless_delete` is unavailable:** use an external-content table
  (`content='fts_docs'`) backed by a `fts_docs` table holding the same six columns. This is decided by
  spike S3 in the [build plan](../build-plan.md).

## 3. Other Durable Objects

```sql
-- DomainMonitor
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys (every key the object reads or writes):
--   schema_version   applied schema (Design § 4, rule 6)
--   domain_id, tenant_id  owner, written by Init and checked on every request (Design § 4)
--   state            the core::domain_fsm state; D1 domains.state is a copy written in the same step
--   candidate, candidate_count  agreed outcome that would change the state, and how many cycles in a row
--                    (Identities and domains › Outcome per resolver and agreement)
--   failing_since    when the domain entered failing (suspension after 14 days)
--   both_error_since the first cycle in a row in which both resolvers returned error; after 6 hours the
--                    cd=1 re-query decides dnssec_bogus or dns_unresolvable (Identities and domains ›
--                    Outcome per resolver and agreement)
--   retired          written by DomainRequest::Retire (the domain_remove finish step) right after
--                    deleteAlarm and deleteAll, so it is the only key left besides schema_version; the
--                    object then answers every request with Retired, before any owner check
--   reminders_sent_json  reminders already sent for the current state; reset on every state change
--   event_seq        outbox sequence (Webhooks › Outbox)
--   outbox_backoff   consecutive failed outbox dispatches, for the retry delay (Webhooks › Dispatching)
--   rdap_pending     {fingerprint, seen_at}: an RDAP change seen once; confirmed by a second query at
--                    least an hour later (alarm:ownership is set to seen_at + 1 hour), cleared otherwise
--   probe:{token}    pending alignment probe {probe_id, sent_at}; dropped after 15 minutes (smtp_probe_timeout)
--   forward:{token}  pending forwarding test {address_id, sent_at}; dropped after 10 minutes (forwarding = failed)
--   role:{hour}, role:{hour}:{sender_hash}  role-mail relay counts per UTC hour (AdmitRoleMail: 30 per domain,
--                    5 per sender; Inbound › Role mail relay); keys older than 48 hours are deleted on write
--   alarm:check, alarm:ownership, alarm:ses_check, alarm:probe, alarm:outbox  pending wake-ups (Design § 4,
--                    rule 5); alarm:probe only for transport = smtp (Domains on any DNS host § 5.3)
-- The NS and RDAP checks compare with D1 (domains.expected_ns_json, domains.rdap_fingerprint), so the
-- object keeps no copy of them.
-- Probe and forwarding-test tokens live only here, never in D1; the result is written to
-- domains.probe_last_at / probe_last_json or addresses.forwarding / forwarding_checked_at.
CREATE TABLE IF NOT EXISTS checks (
  id           INTEGER PRIMARY KEY,
  at           INTEGER NOT NULL,
  resolver     TEXT NOT NULL,                              -- cloudflare-doh | google-doh
  results_json TEXT NOT NULL,                              -- [{record, expected, observed, ok}]
  outcome      TEXT NOT NULL CHECK (outcome IN ('pass','degraded','fail','ownership_changed','error'))
);                                                         -- keeps the last 500 rows
CREATE TABLE IF NOT EXISTS outbox (seq INTEGER PRIMARY KEY, event_id TEXT NOT NULL UNIQUE, type TEXT NOT NULL,
                     payload_json TEXT NOT NULL, occurred_at INTEGER NOT NULL, dispatched_at INTEGER);

-- JobRunner
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys (every key the object reads or writes):
--   schema_version   applied schema (Design § 4, rule 6)
--   job_id, kind, tenant_id  written by Start; tenant_id is the owner checked on every request (Design § 4)
--   event_seq        outbox sequence (Webhooks › Outbox)
--   outbox_backoff   consecutive failed outbox dispatches, for the retry delay (Webhooks › Dispatching)
--   target_address   erasure by address only, from init to finalise (Privacy § 6.4)
--   retry_until      erasure: end of automatic retries, set by Start (Privacy § 4)
--   resume_job_id    erasure: the failed job this one resumes, read by Start (Privacy § 6.1)
--   upload_id        export: the open multipart upload, from init to finish_zip; finalise aborts it (Privacy § 9.1)
--   collect_started_at  export: when collection started, compared by complete (Privacy § 6.11)
--   zip_cd:{n}       export ZIP central-directory entries of batch n, until finalise (Privacy § 9.1)
--   alarm:step, alarm:outbox  pending wake-ups (Design § 4, rule 5)
-- Job status lives in D1 jobs.status, and attempts are per step (steps.attempts).
CREATE TABLE IF NOT EXISTS steps (
  name        TEXT PRIMARY KEY,                            -- e.g. list_r2, delete_vectors, wipe_mailbox
  status      TEXT NOT NULL CHECK (status IN ('pending','running','done','failed','skipped')),
  cursor      TEXT,                                        -- resume point
  counts_json TEXT NOT NULL DEFAULT '{}',
  attempts    INTEGER NOT NULL DEFAULT 0,
  last_error  TEXT,
  updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS held (thread_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, reason TEXT NOT NULL);
                                                           -- erasure: held threads, for receipt.held
CREATE TABLE IF NOT EXISTS deleted_vectors (id TEXT PRIMARY KEY); -- erasure: every vector ID sent to deleteByIds, for
                                                           -- the semantic probe and a resumption (Privacy § 6.7)
CREATE TABLE IF NOT EXISTS outbox (seq INTEGER PRIMARY KEY, event_id TEXT NOT NULL UNIQUE, type TEXT NOT NULL,
                     payload_json TEXT NOT NULL, occurred_at INTEGER NOT NULL, dispatched_at INTEGER);

-- TenantQuota
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys (every key the object reads or writes; nothing is kept in the key-value API):
--   schema_version          applied schema (Design § 4, rule 6)
--   tenant_id               owner, written by QuotaRequest::Init and checked on every request (Security § 5.2)
--   catalog_hash            hash of the plan catalog the allowances came from (SetPlan)
--   billing_mode            metered | exempt | disabled (SetPlan)
--   period_start, period_end  the current billing period, Unix ms (SetPlan, monthly reset)
--   stale:{feature}         '1' after a hold on a count feature expired; the next Hold recounts from D1
--   limit_reached:{feature}:{period_start}  set the first time a Hold is denied in a period
--                           (billing.limit_reached is emitted once per feature and period)
--   alerted:{feature}:{threshold}:{period}  usage alert sent (Notifications § 4); threshold 80 | 100.
--                           sends, triage: {period} = period_start, value '1' (once per period);
--                           counts: {period} = 'count', value = Unix ms of the last alert (24-hour cooldown).
--                           Written when a confirmed hold first crosses the threshold and
--                           NotifierRequest::UsageThreshold is sent; read before sending the next one
--   alarm:holds             earliest holds.expires_at (Billing › Settle, extend and expiry)
--   alarm:reset             earliest allowances.resets_at (Billing › Monthly reset)
--   tm:{msg}                the day (YYYY-MM-DD) on which CountTriageModel counted this message, so a
--                           redelivered triage job is counted once; deleted after 2 days by the daily alarm
--   sm:{msg}                the day on which CountScreenModel counted this message; deleted after 2 days by
--                           the daily alarm (Screening § 5.2). The sender sub-cap counts on the counter
--                           screen_sender:{h} below
--   tz                      the time zone the daily caps use now (IANA); tz_next {zone, from}: a changed
--                           tenants.timezone, applied from `from` = the next local midnight of tz
CREATE TABLE IF NOT EXISTS counters (
  metric TEXT NOT NULL,                                    -- read by GetRates as well as by RecordOutcome
                                                           -- (Deliverability metrics § 3.3).
                                                           -- daily caps: sends, sends:idn_..., agentic,
                                                           --   triage_model (the AI cap of Triage § 12),
                                                           --   screen_model (the screening model cap,
                                                           --   Screening § 5.2), screen_sender:{h} (its
                                                           --   sender sub-cap, h = 16 hex characters of the
                                                           --   sender key's HMAC; deleted after 2 days),
                                                           --   warned:{metric}:{80|100} (sends caps only);
                                                           -- usage: usage:{inbound|outbound|sends|triage|
                                                           --   search|agentic|ai_neurons|assertions|
                                                           --   http_signatures|cf_recipients};
                                                           -- tenant outcomes: outcomes, bounced, complained
                                                           --   (RecordOutcome; summed by OutcomeRates; never
                                                           --   pruned, ForgetIdentity leaves them);
                                                           -- per sending domain, hourly: outcomes:dom:{domain_id},
                                                           --   bounced:dom:{domain_id}, complained:dom:{domain_id}
                                                           --   (RecordOutcome; read by the domain pause, G12, the
                                                           --   domain alerts and the burst pause; windows older
                                                           --   than 8 days deleted by RecordOutcome);
                                                           -- inbound volume: inbound_hour (AdmitInbound, D13;
                                                           --   pruned after 48 hours);
                                                           -- default tenant only: sysmail:{class}:{key}, the
                                                           --   system-mail budgets (SystemMail; Cloud sign-up
                                                           --   § 10.2), keys hashed with PM_HASH_KEY, among them
                                                           --   sysmail:signup:{recipient|human|network|asn|deployment}:{key}
                                                           --   (per UTC day); asu:{outcomes|bounced|complained}
                                                           --   (CountSignupOutcome, per UTC hour, pruned after
                                                           --   48 hours; Agent self-sign-up § 13.3); screen_fail
                                                           --   (CountScreenFailure, per UTC hour, pruned after
                                                           --   48 hours; Screening § 22)
  window TEXT NOT NULL,                                    -- YYYY-MM-DD: the tenant's time zone (tz above) for
                                                           -- daily caps, UTC for usage:* (flushed to usage_daily),
                                                           -- for the outcome counters and for sysmail:*;
                                                           -- YYYY-MM-DDTHH (UTC hour) for inbound_hour, the
                                                           -- per-domain outcome counters, asu:* and screen_fail
  value  INTEGER NOT NULL,
  PRIMARY KEY (metric, window)
);
CREATE TABLE IF NOT EXISTS outcome_events (                -- RecordOutcome (and CountSignupOutcome) is idempotent
                                                           -- by event ID: a retried
  event_id TEXT PRIMARY KEY,                               -- delivery event is counted once (Outbound › Abuse
  at       INTEGER NOT NULL                                -- auto-pause); rows older than 7 days are deleted by
);                                                         -- the daily alarm
CREATE TABLE IF NOT EXISTS allowances (                    -- plan + top-ups for the current period
  feature    TEXT PRIMARY KEY CHECK (feature IN ('inboxes','sends','triage','custom_domains','storage_gb','seats')),
  granted    INTEGER,                                      -- NULL = unlimited (exempt / disabled)
  used       INTEGER NOT NULL DEFAULT 0,                   -- consumed this period (or current count)
  held       INTEGER NOT NULL DEFAULT 0,                   -- units in open holds
  resets_at  INTEGER                                       -- NULL for counts that never reset
);
CREATE TABLE IF NOT EXISTS holds (
  id         TEXT PRIMARY KEY,                             -- hld_
  feature    TEXT NOT NULL,
  units      INTEGER NOT NULL,
  ref        TEXT NOT NULL,                                -- e.g. msg_… or idn_…; one open hold per (feature, ref)
  expires_at INTEGER NOT NULL,                             -- created or last extended + 10 minutes; the alarm releases it
  UNIQUE (feature, ref)
);
CREATE TABLE IF NOT EXISTS outcomes (                      -- sliding windows for abuse thresholds
  identity_id TEXT NOT NULL,
  seq         INTEGER NOT NULL,
  outcome     TEXT NOT NULL CHECK (outcome IN ('delivered','bounced','complained','other')),
  at          INTEGER NOT NULL,
  PRIMARY KEY (identity_id, seq)
);                                                         -- keeps the last 1,000 per identity

-- SesControl (one per deployment, only with SES; Domains on any DNS host § 4.8)
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys: schema_version; next_free_ms (the earliest time the next SES control-plane call may start);
--   alarm:rule_sync (the next attempt at the oldest change set); identities_total, identities_read_at
--   (the region's ListEmailIdentities total, read once a UTC day by the platform check)
-- The single writer of the pm-retired-{n} receipt rules (Domains on any DNS host § 4.6): queued
-- SyncRetired change sets, worked oldest first and deleted once a read-back matches.
CREATE TABLE IF NOT EXISTS rule_sync (
  seq        INTEGER PRIMARY KEY,
  add_json   TEXT NOT NULL,                                -- address IDs to add to a pm-retired-{n} rule
  remove_json TEXT NOT NULL,                               -- address IDs to remove
  attempts   INTEGER NOT NULL DEFAULT 0,
  queued_at  INTEGER NOT NULL
);

-- Notifier (one per tenant; Notifications § 8). Holds user, identity and message IDs and counts, never mail content.
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys (every key the object reads or writes):
--   schema_version   applied schema (Design § 4, rule 6)
--   tenant_id        owner, written by NotifierRequest::Init and checked on every request (Design § 5)
--   alarm:send       earliest pending.due_at (Design § 4, rule 5)
--   alarm:held       earliest held.until: a message waiting for triage is counted when it passes
--   alarm:daily      the next 09:00 in the tenant's time zone (tenants.timezone): the needs_person email,
--                    daily new_mail emails and the digest of capped items (kind digest)
--   prefs_cache_at   when the cached notification_prefs of the workspace were last read from D1
CREATE TABLE IF NOT EXISTS pending (                       -- items waiting for their window
  user_id           TEXT NOT NULL,                         -- usr_
  kind              TEXT NOT NULL CHECK (kind IN ('usage','new_mail','needs_person','account','digest')),
  ref               TEXT NOT NULL DEFAULT '-',             -- new_mail: the identity ID (inbox); usage:
                                                           -- '{feature}:{threshold}'; account: the event; else '-'
                                                           -- (digest: items held back by a cap, sent at 09:00)
  count             INTEGER NOT NULL DEFAULT 0,            -- messages (new_mail) or items
  needs_reply_count INTEGER NOT NULL DEFAULT 0,            -- new_mail: of which waiting for a reply
  detail_json       TEXT,                                  -- usage: {feature, threshold, used, granted, period};
                                                           -- account: {event, tenant_id}; digest: counts per kind
                                                           -- and inbox or threshold; never mail content
  first_at          INTEGER NOT NULL,
  due_at            INTEGER NOT NULL,                      -- when the window closes (or the next hourly retry)
  attempts          INTEGER NOT NULL DEFAULT 0,            -- send attempts while the platform domain fails
                                                           -- (hourly, for at most 24 hours)
  PRIMARY KEY (user_id, kind, ref)
);
CREATE INDEX IF NOT EXISTS pending_due ON pending(due_at);
CREATE TABLE IF NOT EXISTS held (                          -- new_mail with filter = needs_reply: waiting for triage
  message_id  TEXT NOT NULL,                               -- msg_
  identity_id TEXT NOT NULL,                               -- idn_ (the inbox)
  user_id     TEXT NOT NULL,                               -- usr_ of a person with that filter following the inbox
  until       INTEGER NOT NULL,                            -- arrival + 5 minutes; then the message is counted
  PRIMARY KEY (message_id, user_id)
);                                                         -- deleted on message.triaged (counted if needs_reply >= 0.5,
                                                           -- else dropped), at until (counted), or on MemberRemoved
CREATE INDEX IF NOT EXISTS held_until ON held(until);
CREATE TABLE IF NOT EXISTS windows (                       -- last send, for the 10-minute rule of instant mode
  user_id      TEXT NOT NULL,
  kind         TEXT NOT NULL,
  ref          TEXT NOT NULL DEFAULT '-',                  -- as pending.ref
  last_sent_at INTEGER NOT NULL,                           -- written after each send; read by the 10-minute rule;
                                                           -- rows older than 1 day are deleted by the daily alarm
  PRIMARY KEY (user_id, kind, ref)
);
CREATE TABLE IF NOT EXISTS sent (                          -- per-day counters for the caps (50 per person, 200 per
  day     TEXT NOT NULL,                                   -- workspace); YYYY-MM-DD in the tenant's time zone
  user_id TEXT NOT NULL,                                   -- usr_, or '*' for the workspace total
  count   INTEGER NOT NULL,
  PRIMARY KEY (day, user_id)
);                                                         -- rows older than 2 days are deleted by the daily alarm

-- TenantEvents (one per tenant; Event feed and live stream § 2.2).
-- TenantEvents schema v1 (crates/worker/src/events/schema/). Holds event envelopes (thin payloads: IDs,
-- summaries, verdicts and at most policy.webhook_text_bytes of extracted text), consumer positions and
-- stream-token hashes. Never raw MIME, attachments or verification values.
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
-- keys (every key the object reads or writes):
--   schema_version     applied schema (Design § 4, rule 6)
--   tenant_id          owner, written by EventsRequest::Init and checked on every request (Design § 5)
--   created_at         written by Init
--   epoch              16 random bytes, lower-case hex; written by Init and by Reset; part of every cursor
--   cursor_secret      32 random bytes, base64; the HMAC key of this log's cursors; written by Init and by
--                      Reset; never leaves the object
--   head_seq           the last position assigned in this epoch (0 after Init or Reset); never decreases
--   floor_seq          the highest position removed by retention or by the size budget (0 if none). A cursor
--                      below it has expired. Erasure never raises it
--   floor_reason       retention | size_limit: why floor_seq last moved
--   floor_appended_at  the largest appended_at that retention or the size budget removed (0 if none); a
--                      since at or before it reports since_truncated (Event feed § 5.2)
--   payload_bytes      sum of length(envelope_json) over the events rows, kept in the same transaction as
--                      every insert and delete
--   log_bytes          sum of length(envelope_json) + 1024 over the events and staged_appends rows: the
--                      size budget's measure (Event feed § 2.5), kept in the same transactions as payload_bytes
--   last_appended_at   appended_at of the newest row; appended_at never decreases
--   backfilling        '1' from a rebuild's reset until its backfill finishes (Event feed § 9.3); live appends
--                      are staged meanwhile
--   backfill_at        when the backfill last appended (or the reset ran); read by the maintenance alarm
--   erased             '1' after EventsRequest::Erase (tenant erasure): every later request gets tenant_not_found
--   alarm:maintenance  daily, hourly while backfilling, and at once when the size budget is passed
--                      (Event feed § 2.6)
--   alarm:revalidate   every 60 s while a WebSocket is attached (Event feed § 7.7)
CREATE TABLE IF NOT EXISTS events (
  seq           INTEGER PRIMARY KEY,                       -- log position: head_seq + 1 at append
  event_id      TEXT NOT NULL UNIQUE,                      -- evt_; the deduplication key of Append
  type          TEXT NOT NULL,
  identity_id   TEXT,                                      -- the envelope's identity_id
  message_id    TEXT,                                      -- data.message_id, else data.message.id (erasure)
  thread_id     TEXT,                                      -- data.thread_id, else data.message.thread_id (holds)
  occurred_at   INTEGER NOT NULL,                          -- the envelope's occurred_at: the outbox commit time
  appended_at   INTEGER NOT NULL,                          -- max(now, meta.last_appended_at); retention counts
                                                           -- from it
  envelope_json TEXT NOT NULL                              -- the exact envelope bytes the owner's outbox holds,
                                                           -- which are also the webhook body
);
CREATE INDEX IF NOT EXISTS events_identity ON events(identity_id, seq) WHERE identity_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS events_message ON events(message_id) WHERE message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS events_appended ON events(appended_at);
CREATE TABLE IF NOT EXISTS event_messages (                -- the messages an event lists in data.messages[]
  message_id TEXT NOT NULL,                                -- (message.labels_changed), so erasure finds it
  seq        INTEGER NOT NULL REFERENCES events(seq) ON DELETE CASCADE,
  PRIMARY KEY (message_id, seq)
);
CREATE TABLE IF NOT EXISTS staged_appends (                -- live appends held while a rebuild backfills
  event_id      TEXT PRIMARY KEY,                          -- (Event feed § 9.3); emptied when the backfill finishes
  identity_id   TEXT,
  message_ids   TEXT NOT NULL,                             -- JSON array: message_id and data.messages[] IDs
  occurred_at   INTEGER NOT NULL,
  staged_at     INTEGER NOT NULL,
  envelope_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS consumers (                     -- durable consumers (Event feed § 6)
  id                TEXT PRIMARY KEY,                      -- evc_
  name              TEXT NOT NULL UNIQUE,                  -- unique per tenant: one object per tenant
  owner_key_id      TEXT NOT NULL,                         -- key_ that created it: the only key that reads,
                                                           -- commits and resets it
  owner_identity_id TEXT,                                  -- the owner key's identity, for an identity key
  types_json        TEXT NOT NULL,                         -- ["*"] or type patterns (Event feed § 4)
  identity_ids_json TEXT,                                  -- NULL: every identity the owner key may see
  committed_seq     INTEGER NOT NULL,                      -- events at or below are acknowledged
  epoch             TEXT NOT NULL,                         -- meta.epoch when committed_seq was set; another
                                                           -- epoch means reset_required
  committed_at      INTEGER,
  last_read_at      INTEGER,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS consumers_owner ON consumers(owner_key_id);
CREATE TABLE IF NOT EXISTS stream_tokens (                 -- single-use stream tokens (Event feed § 7.2)
  token_hash  TEXT PRIMARY KEY,                            -- hex SHA-256 of the token's 32 random bytes
  key_id      TEXT NOT NULL,                               -- the minting key: the stream acts as this key
  expires_at  INTEGER NOT NULL                             -- minted + 60 s; deleted on redemption, by the next
                                                           -- mint once expired, or by the maintenance alarm
);
CREATE TABLE IF NOT EXISTS erased_refs (                   -- refuses late appends of erased data (R57, R58)
  ref        TEXT PRIMARY KEY,                             -- msg_… or idn_…
  erased_at  INTEGER NOT NULL                              -- deleted by the maintenance alarm after 7 days
);
-- Per-connection state (wsc_ connection IDs, positions, filters) is the WebSocket's attachment, not a row
-- (Event feed § 7.9).
```

## 4. R2 objects

| Key | Content | Custom metadata | Deleted by |
|---|---|---|---|
| `inbound-staging/{yyyy}/{mm}/{dd}/{ulid}.eml` | Raw message before routing resolves | `envelope_to_hash`, `envelope_from`, `envelope_to`, `received_at` | The inbound consumer after the move, or the lifecycle rule (15 days, longer than the 14-day dead-letter retention, [J7](../edge-cases.md)) |
| `inbound-staging/ses/{key}` | Raw message received through SES, copied from S3 (`in/{key}`) before its recipients are resolved | – | The lifecycle rule (15 days); a held message whose copy is gone is fetched from S3 again |
| `inbound-staging/role/{yyyy}/{mm}/{dd}/{ulid}.eml` | Role mail (`postmaster@`, `abuse@`, …) waiting to be relayed through the system identity ([Inbound › Role mail relay](inbound.md#role-mail-relay)) | – | The inbound consumer after the relay, or the lifecycle rule (15 days on `inbound-staging/`) |
| `t/{ten}/i/{idn}/m/{msg}/raw.eml` | Raw inbound MIME | `tenant`, `identity`, `message`, and from `email()` `envelope_from` and `envelope_to`, which a restore's `restore_reconcile` job re-queues the pointer with | Retention (`raw_days`), erasure |
| `t/{ten}/i/{idn}/m/{msg}/a/{att}` | Attachment bytes | same, plus `sha256` | Erasure, message retention |
| `t/{ten}/i/{idn}/m/{msg}/a/{att}.md` | Extracted text (Markdown, with page markers) | same | as above |
| `t/{ten}/i/{idn}/out/{msg}.eml` | Composed outbound MIME (sent copy) | same, plus `idem_key_sha256` (hex SHA-256 of the Idempotency-Key), `fingerprint` and `operation` | Retention, erasure; the submit path when the send is refused after it was written; for the system identity, as soon as the message leaves `queued` |
| `t/{ten}/i/{idn}/out/{msg}/a/{att}` | Outbound attachment bytes (linked attachments, copies for `GET …/attachments/{id}`, and copies of uploads) | `tenant`, `identity`, `message`, `sha256` | Retention, erasure |
| `t/{ten}/i/{idn}/up/{upl}` | Upload bytes, verified against the declared SHA-256 ([Uploads § 5.1](uploads.md#51-mailbox-table)) | `tenant`, `identity`, `upload`, `sha256` | Rejection, expiry, `DELETE`, erasure; the orphan sweep after 15 days when no row names it |
| `t/{ten}/i/{idn}/drafts/{drf}/a/{att}` | A draft attachment: an inline part, or the copy of an upload ([Drafts § 9.4](drafts.md#94-r2)) | – | When it is removed by `PATCH`, when the draft is sent or canceled (after commit; for a held draft, when the hold ends), by erasure and by the retention purge of its draft; the orphan sweep removes uncommitted ones |
| `t/{ten}/exports/{exp}.zip` | Subject-access export | `tenant`, `export` | 7 days after creation |
| `phone-staging/{yyyy}/{mm}/{dd}/{event_id}.json` | A raw provider event (written by: `POST /hooks/telnyx`) | – | The consumer after processing; a lifecycle rule after 15 days |
| `t/{ten}/phone/docs/{cpd}` | A KYC document, AES-256-GCM under its sealed data key (written by: The upload link) | – | Document delete, retention, tenant erasure |
| `t/{ten}/i/{idn}/sms/{sms}/m/{n}` | An inbound MMS item (written by: The `pm-phone` consumer) | – | Retention and erasure, with its SMS |
| `t/{tenant_id}/vault/{item_id}/{version}` | A session version's `session_state` ciphertext (AES-256-GCM under the version's DEK), at most 1 MiB of plaintext (written by: Session create, import and write-back) | – | Write-back of the next version, expiry, item deletion, erasure, the daily orphan sweep. Never copied to `PM_BACKUP_BUCKET` |

For Email Routing, `email()` writes straight to the final key when routing resolved (the normal case);
the dated staging key is used only when the directory lookup fails transiently and the message is accepted
for later routing. Every message received through SES is staged, because its recipients are resolved in
the consumer, not at receipt: the consumer copies `in/{key}` from S3 to `inbound-staging/ses/{key}`, then
to each recipient's final key ([Inbound › The SES source](inbound.md#the-ses-source)).

The metadata on `out/{msg}.eml` lets a point-in-time restore of a mailbox rebuild the idempotency
ledger for sends made after the restore point ([Observability › Restore from PITR](observability.md#restore-from-pitr)).

Every object under `t/` is named by a row; one that is not (an orphan) is deleted at its source, or by the
mailbox's rolling orphan sweep once it is 15 days old ([Inbound › Orphan objects](inbound.md#orphan-objects),
[I9](../edge-cases.md)).

**Optional backup bucket.** When `PM_BACKUP_BUCKET` is set, the nightly `backup` job copies every `t/`
object created since its last run to the same key in that bucket (binding `BACKUP`, same jurisdiction),
except keys under `…/up/` (uploads are a staging area that empties within 7 days).
Retention and erasure delete each key from both buckets. See [Privacy › R2 backup copy](privacy.md#54-optional-r2-backup-copy).

## 5. Vectorize

```text
index:       pm-mail-chunks for generation 1, then pm-mail-chunks-g{N} for generation N ≥ 2, one per
             embedding model (Search § 7.3); one generation in use per deployment, two during a re-embed;
             staging has its own
dimensions:  1024 for generation 1 (@cf/baai/bge-m3); a later generation's are probed from its model by
             embedding a test string before the index is created (CLI and setup § 8.7)
metric:      cosine
namespace:   tenant id (ten_…, ≤ 64 bytes)
vector id:   {message_id}:{n}  or  {message_id}:a{k}:{n}   (≤ 64 bytes)
metadata indexes (8 of 10 allowed):
  identity_id     string
  thread_id       string
  sent_at         number   (unix seconds)
  sender_domain   string
  direction       string   (inbound | outbound)
  has_attachment  boolean
  verdict         string   (the messages.verdict values, arc_rescued included)
  kind            string   (body | attachment)
metadata stored: the indexed fields only. Never text, subject or addresses.
```

Create it with `idvy setup`, which calls the Vectorize API for the index and each metadata index.
Query with `topK ≤ 100` and `returnMetadata: "none"`: the message ID is parsed from the vector ID, and
text is always read back from the mailbox, which also enforces visibility (quarantine, holds, erasure).
