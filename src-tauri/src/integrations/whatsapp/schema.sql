-- WhatsApp protocol store (Plan 023 Step 2).
--
-- This database is separate from Alfred's app.db and contains no plaintext
-- identity. Every `k`/`*_k` column is a keyed HMAC-SHA256 digest and every `v`
-- column is a ChaCha20-Poly1305 envelope bound to its row. Namespace strings and
-- timestamps are the only plaintext: they are schema and retention metadata, not
-- user data, and the expiry sweeps need to range over them.
--
-- Deliberately absent: conversations, inbound message content, decoded
-- history-sync blobs, contacts, media, profiles, and analytics.

PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;

-- Generic keyed-blob storage: identities, sessions, sender keys, app-state
-- versions, device lists, and retry base keys.
CREATE TABLE IF NOT EXISTS kv (
    ns TEXT NOT NULL,
    k  BLOB NOT NULL,
    v  BLOB NOT NULL,
    PRIMARY KEY (ns, k)
) WITHOUT ROWID;

-- The single linked device. `id` is the protocol's own device id, not a secret.
CREATE TABLE IF NOT EXISTS device (
    id INTEGER PRIMARY KEY,
    v  BLOB NOT NULL
);

-- Pre-key ids are sequential protocol counters, not identity, and MAX(id) plus
-- the uploaded flag both need to be queryable.
CREATE TABLE IF NOT EXISTS prekeys (
    id       INTEGER PRIMARY KEY,
    v        BLOB NOT NULL,
    uploaded INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS signed_prekeys (
    id INTEGER PRIMARY KEY,
    v  BLOB NOT NULL
);

-- Per-group sender-key distribution tracking. The device JID is stored sealed
-- because callers need it back verbatim.
CREATE TABLE IF NOT EXISTS sender_key_devices (
    group_k   BLOB NOT NULL,
    device_k  BLOB NOT NULL,
    device_v  BLOB NOT NULL,
    has_key   INTEGER NOT NULL,
    PRIMARY KEY (group_k, device_k)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_skd_device ON sender_key_devices (device_k);

-- LID <-> phone-number mapping. Both sides are digested so either direction can
-- be looked up without either identifier appearing in the clear.
CREATE TABLE IF NOT EXISTS lid_map (
    lid_k      BLOB PRIMARY KEY,
    pn_k       BLOB NOT NULL,
    v          BLOB NOT NULL,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_lid_map_pn ON lid_map (pn_k, updated_at DESC);

-- App-state sync keys. `key_id_v` is sealed because `get_latest_sync_key_id`
-- must return the raw id; `updated_at` orders "latest".
CREATE TABLE IF NOT EXISTS sync_keys (
    k          BLOB PRIMARY KEY,
    key_id_v   BLOB NOT NULL,
    v          BLOB NOT NULL,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS mutation_macs (
    name_k  BLOB NOT NULL,
    index_k BLOB NOT NULL,
    v       BLOB NOT NULL,
    version INTEGER NOT NULL,
    PRIMARY KEY (name_k, index_k)
) WITHOUT ROWID;

-- Trusted-contact tokens. The timestamps stay plaintext so the expiry sweep can
-- range over them without decrypting every row.
CREATE TABLE IF NOT EXISTS tc_tokens (
    k         BLOB PRIMARY KEY,
    jid_v     BLOB NOT NULL,
    v         BLOB NOT NULL,
    token_ts  INTEGER NOT NULL,
    sender_ts INTEGER
) WITHOUT ROWID;

-- Outbound retry payloads. `created_at` drives the hard 24-hour purge; these
-- rows are the only place a resolved outgoing message body may persist, and
-- never as user-visible history.
CREATE TABLE IF NOT EXISTS sent_messages (
    k          BLOB PRIMARY KEY,
    v          BLOB NOT NULL,
    created_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_sent_messages_created ON sent_messages (created_at);

CREATE TABLE IF NOT EXISTS msg_secrets (
    k          BLOB PRIMARY KEY,
    v          BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    message_ts INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_msg_secrets_expiry ON msg_secrets (expires_at);
