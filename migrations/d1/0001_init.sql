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
  key        TEXT PRIMARY KEY CHECK (key IN ('send_breaker','phone_account')),
  value_json TEXT NOT NULL,                                -- send_breaker: {"stage": 1 | 2, "until": <ms>, "share": <0..1>}
                                                           -- phone_account: {"level": "paid" | "verified" | "unknown",
                                                           -- "probed_at": <ms>, "us_enabled": bool}, written by
                                                           -- POST /v1/platform/phone/probe (Phone numbers § 2.1)
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
