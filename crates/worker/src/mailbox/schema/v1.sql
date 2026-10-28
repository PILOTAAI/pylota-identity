-- IdentityMailbox schema v1 (Data model § 2, generated from it; keep the two equal: the test
-- `mailbox::tests::schema_matches_data_model` compares them). Every statement is idempotent (Design
-- conventions § 4, rule 6). `PRAGMA foreign_keys = ON` is applied by `Mailbox::migrate` before the
-- migration's transaction, where SQLite ignores it.

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
