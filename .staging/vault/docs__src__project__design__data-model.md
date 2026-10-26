Where this goes: `docs/src/project/design/data-model.md`. (1) Five ID prefixes into the Conventions table; (2) the D1 tables at the end of "## 1. D1 control plane" (before "### Notes"); (3) a column on `service_accounts`; (4) three columns on the mailbox `verifications` table and a quarantine reason; (5) one R2 row. Source: [Vault](vault.md).

## 1. ID prefixes

| `vit_` | vault item | `vgr_` | vault grant |
| `vus_` | vault use | `vcn_` | vault consumer |
| `vad_` | vault audit row | `vtk_` | vault ticket (never stored; its SHA-256 only) |

## 2. D1 tables

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

-- Uses (Vault § 6, § 7). ticket_hash and dek_ticket_sealed are cleared when the use ends.
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
  run_id            TEXT,
  details_json      TEXT NOT NULL DEFAULT '{}',             -- counts and enumerations only
  created_at        INTEGER NOT NULL
);
CREATE UNIQUE INDEX vault_audit_seq ON vault_audit(tenant_id, seq);
CREATE INDEX vault_audit_item ON vault_audit(tenant_id, item_id, seq);
CREATE INDEX vault_audit_created ON vault_audit(created_at);

-- Audit anchors (Vault § 16.1): the last row retention deleted, so the chain stays verifiable.
CREATE TABLE vault_audit_anchors (
  tenant_id  TEXT PRIMARY KEY,
  seq        INTEGER NOT NULL,
  row_hash   TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

Writers and readers: every table above is written by the vault handlers and jobs named in
[Vault](vault.md) (§ 3–§ 16) and read by the same routes, the consumer routes, the every-minute vault cron, the
daily vault sweep, the global retention job's `vault` step and the erasure steps.

## 3. `service_accounts`

```sql
  login_domains_json  TEXT NOT NULL DEFAULT '[]',          -- 0–5 organisational domains where the account
                                                            -- signs in (Vault § 3.3); read by the vault's origin rule
```

## 4. Mailbox `verifications`

Add to the `verifications` table:

```sql
  vault_item_id     TEXT,                                   -- set by quarantine rule 4b (Vault § 9.2)
  vault_use_id      TEXT,                                   -- set by VaultTakeCode
  vault_consumed_at INTEGER,                                -- set by VaultTakeCode; a consumed code is never released
```

and `'vault_code'` to the `quarantine_reason` `CHECK` of `messages`.

## 5. R2

| Key | Holds | Written by | Deleted by |
|---|---|---|---|
| `t/{tenant_id}/vault/{item_id}/{version}` | A session version's `session_state` ciphertext (AES-256-GCM under the version's DEK), at most 1 MiB of plaintext | Session create, import and write-back | Write-back of the next version, expiry, item deletion, erasure, the daily orphan sweep. Never copied to `PM_BACKUP_BUCKET` |
