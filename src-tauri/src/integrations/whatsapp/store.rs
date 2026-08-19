//! Alfred-owned encrypted protocol store for the WhatsApp linked device
//! (Plan 023 Step 2).
//!
//! Implements `wacore`'s `Backend` trait surface over Alfred's existing rusqlite
//! linkage against a dedicated database file. The bundled `SqliteStore` is not
//! usable here: it is Diesel on `libsqlite3-sys 0.37`, which cannot coexist with
//! Alfred's `libsqlite3-sys 0.38.2` (see the Step 1 spike results).
//!
//! Every sensitive lookup key is a keyed digest and every sensitive value is a
//! sealed envelope — see [`super::crypto`]. Nothing here persists conversations,
//! inbound content, decoded history-sync blobs, contacts, media, or profiles.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wacore::appstate::hash::HashState;
use wacore::store::error::{Result as StoreResult, StoreError};
use wacore::store::traits::{
    merge_msg_secret_expiry, merge_msg_secret_message_ts, AppStateSyncKey, AppSyncStore,
    DeviceListRecord, DeviceStore, LidPnMappingEntry, MsgSecretEntry, MsgSecretStore,
    ProtocolStore, SignalStore, TcTokenEntry,
};
use wacore::store::Device;
use wacore_appstate::processor::AppStateMutationMAC;

use super::crypto::{row_aad, StoreKey};

/// Plan 023 caps outbound retry payload retention. The store clamps to this
/// regardless of what a caller asks for, so no caller can widen the window.
const MAX_RETRY_RETENTION_SECONDS: i64 = 24 * 60 * 60;

/// Namespaces for the generic `kv` table. Also the digest domain separators, so
/// the same address under two namespaces yields unrelated digests.
mod ns {
    pub const IDENTITY: &str = "identity";
    pub const SESSION: &str = "session";
    pub const SENDER_KEY: &str = "sender_key";
    pub const APP_VERSION: &str = "app_version";
    pub const DEVICE_LIST: &str = "device_list";
    pub const BASE_KEY: &str = "base_key";
    // Digest-only domains (their rows live in dedicated tables).
    pub const SKD_GROUP: &str = "skd_group";
    pub const SKD_DEVICE: &str = "skd_device";
    pub const LID: &str = "lid";
    pub const PN: &str = "pn";
    pub const SYNC_KEY: &str = "sync_key";
    pub const MAC_NAME: &str = "mac_name";
    pub const MAC_INDEX: &str = "mac_index";
    pub const TC_TOKEN: &str = "tc_token";
    pub const SENT_MESSAGE: &str = "sent_message";
    pub const MSG_SECRET: &str = "msg_secret";
}

fn db_err<E: std::error::Error + Send + Sync + 'static>(error: E) -> StoreError {
    StoreError::Database(Box::new(error))
}

fn ser_err<E: std::error::Error + Send + Sync + 'static>(error: E) -> StoreError {
    StoreError::Serialization(Box::new(error))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One paired account's protocol state, encrypted at rest.
pub struct EncryptedProtocolStore {
    conn: Mutex<Connection>,
    key: Arc<StoreKey>,
    path: PathBuf,
}

impl EncryptedProtocolStore {
    /// Opens (creating if absent) the protocol database at `path` under `key`.
    ///
    /// The file is created with owner-only permissions. A wrong key is not
    /// detected here — it surfaces as an authentication failure on the first
    /// read, which is the correct behaviour for an AEAD store.
    pub fn open(path: impl AsRef<Path>, key: StoreKey) -> StoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            restrict_permissions(parent, 0o700)?;
        }

        let conn = Connection::open(&path).map_err(db_err)?;
        conn.execute_batch(include_str!("schema.sql"))
            .map_err(db_err)?;
        restrict_permissions(&path, 0o600)?;

        Ok(Self {
            conn: Mutex::new(conn),
            key: Arc::new(key),
            path,
        })
    }

    #[cfg(test)]
    fn open_in_memory(key: StoreKey) -> StoreResult<Self> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        // WAL is meaningless for an in-memory database and SQLite rejects it.
        conn.execute_batch(
            &include_str!("schema.sql")
                .replace("PRAGMA journal_mode = WAL;", "")
                .replace("PRAGMA synchronous = FULL;", ""),
        )
        .map_err(db_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
            key: Arc::new(key),
            path: PathBuf::from(":memory:"),
        })
    }

    /// Deletes the database and every sidecar file SQLite may have written.
    /// Used by the disconnect path, which must clear local state whether or not
    /// the remote logout succeeded.
    pub fn delete_files(path: impl AsRef<Path>) -> std::io::Result<u32> {
        let path = path.as_ref();
        let mut removed = 0;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
            match std::fs::remove_file(&candidate) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(removed)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` against the connection. The guard never crosses an await point:
    /// every statement in this module is synchronous.
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> StoreResult<T> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| StoreError::Validation("protocol store lock poisoned".into()))?;
        f(&conn).map_err(db_err)
    }

    fn digest(&self, namespace: &str, value: &[u8]) -> [u8; 32] {
        self.key.digest(namespace, value)
    }

    fn seal(&self, namespace: &str, key_digest: &[u8], plaintext: &[u8]) -> Vec<u8> {
        self.key.seal(&row_aad(namespace, key_digest), plaintext)
    }

    fn open_value(
        &self,
        namespace: &str,
        key_digest: &[u8],
        envelope: &[u8],
    ) -> StoreResult<Vec<u8>> {
        self.key
            .open(&row_aad(namespace, key_digest), envelope)
            .map_err(ser_err)
    }

    // --- generic kv helpers -------------------------------------------------

    fn kv_put(&self, namespace: &str, key: &[u8], value: &[u8]) -> StoreResult<()> {
        let k = self.digest(namespace, key);
        let v = self.seal(namespace, &k, value);
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO kv (ns, k, v) VALUES (?1, ?2, ?3)
                 ON CONFLICT(ns, k) DO UPDATE SET v = excluded.v",
                params![namespace, &k[..], v],
            )
            .map(|_| ())
        })
    }

    fn kv_get(&self, namespace: &str, key: &[u8]) -> StoreResult<Option<Vec<u8>>> {
        let k = self.digest(namespace, key);
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v FROM kv WHERE ns = ?1 AND k = ?2",
                params![namespace, &k[..]],
                |row| row.get(0),
            )
            .optional()
        })?;
        stored
            .map(|envelope| self.open_value(namespace, &k, &envelope))
            .transpose()
    }

    fn kv_delete(&self, namespace: &str, key: &[u8]) -> StoreResult<()> {
        let k = self.digest(namespace, key);
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM kv WHERE ns = ?1 AND k = ?2",
                params![namespace, &k[..]],
            )
            .map(|_| ())
        })
    }

    fn kv_put_json<T: Serialize>(&self, namespace: &str, key: &[u8], value: &T) -> StoreResult<()> {
        let encoded = serde_json::to_vec(value).map_err(ser_err)?;
        self.kv_put(namespace, key, &encoded)
    }

    fn kv_get_json<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &[u8],
    ) -> StoreResult<Option<T>> {
        match self.kv_get(namespace, key)? {
            Some(raw) => Ok(Some(serde_json::from_slice(&raw).map_err(ser_err)?)),
            None => Ok(None),
        }
    }

    /// Purges every expiring row. Called at startup, after sends, and on the
    /// maintenance interval. Retry payloads are clamped to the plan's hard
    /// 24-hour ceiling regardless of the caller's cutoff.
    pub async fn purge_expired(&self) -> StoreResult<u32> {
        let now = now_secs();
        let retries = self.delete_expired_sent_messages(now).await?;
        let secrets = self.delete_expired_msg_secrets(now).await?;
        Ok(retries + secrets)
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path, _mode: u32) -> std::io::Result<()> {
    // Windows inherits the per-user ACL of the app-data directory, which is
    // already owner-scoped. Nothing further to tighten here.
    Ok(())
}

#[async_trait]
impl SignalStore for EncryptedProtocolStore {
    async fn put_identity(&self, address: &str, key: [u8; 32]) -> StoreResult<()> {
        self.kv_put(ns::IDENTITY, address.as_bytes(), &key)
    }

    async fn load_identity(&self, address: &str) -> StoreResult<Option<[u8; 32]>> {
        match self.kv_get(ns::IDENTITY, address.as_bytes())? {
            Some(raw) => {
                let key: [u8; 32] = raw.as_slice().try_into().map_err(|_| {
                    StoreError::Validation(format!("identity key length {} is invalid", raw.len()))
                })?;
                Ok(Some(key))
            }
            None => Ok(None),
        }
    }

    async fn delete_identity(&self, address: &str) -> StoreResult<()> {
        self.kv_delete(ns::IDENTITY, address.as_bytes())
    }

    async fn get_session(&self, address: &str) -> StoreResult<Option<Bytes>> {
        Ok(self
            .kv_get(ns::SESSION, address.as_bytes())?
            .map(Bytes::from))
    }

    async fn put_session(&self, address: &str, session: &[u8]) -> StoreResult<()> {
        self.kv_put(ns::SESSION, address.as_bytes(), session)
    }

    async fn delete_session(&self, address: &str) -> StoreResult<()> {
        self.kv_delete(ns::SESSION, address.as_bytes())
    }

    async fn store_prekey(&self, id: u32, record: &[u8], uploaded: bool) -> StoreResult<()> {
        let k = self.digest("prekey", &id.to_le_bytes());
        let v = self.seal("prekey", &k, record);
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO prekeys (id, v, uploaded) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET v = excluded.v, uploaded = excluded.uploaded",
                params![id, v, uploaded as i64],
            )
            .map(|_| ())
        })
    }

    async fn load_prekey(&self, id: u32) -> StoreResult<Option<Bytes>> {
        let k = self.digest("prekey", &id.to_le_bytes());
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row("SELECT v FROM prekeys WHERE id = ?1", params![id], |row| {
                row.get(0)
            })
            .optional()
        })?;
        stored
            .map(|envelope| self.open_value("prekey", &k, &envelope).map(Bytes::from))
            .transpose()
    }

    async fn mark_prekeys_uploaded(&self, ids: &[u32]) -> StoreResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        // UPDATE, never upsert: a pre-key consumed between the upload snapshot
        // and this call must stay deleted.
        self.with_conn(|conn| {
            let mut statement = conn.prepare("UPDATE prekeys SET uploaded = 1 WHERE id = ?1")?;
            for id in ids {
                statement.execute(params![id])?;
            }
            Ok(())
        })
    }

    async fn remove_prekey(&self, id: u32) -> StoreResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM prekeys WHERE id = ?1", params![id])
                .map(|_| ())
        })
    }

    async fn get_max_prekey_id(&self) -> StoreResult<u32> {
        self.with_conn(|conn| {
            conn.query_row("SELECT COALESCE(MAX(id), 0) FROM prekeys", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .map(|max| max as u32)
    }

    async fn store_signed_prekey(&self, id: u32, record: &[u8]) -> StoreResult<()> {
        let k = self.digest("signed_prekey", &id.to_le_bytes());
        let v = self.seal("signed_prekey", &k, record);
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO signed_prekeys (id, v) VALUES (?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET v = excluded.v",
                params![id, v],
            )
            .map(|_| ())
        })
    }

    async fn load_signed_prekey(&self, id: u32) -> StoreResult<Option<Vec<u8>>> {
        let k = self.digest("signed_prekey", &id.to_le_bytes());
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v FROM signed_prekeys WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
        })?;
        stored
            .map(|envelope| self.open_value("signed_prekey", &k, &envelope))
            .transpose()
    }

    async fn load_all_signed_prekeys(&self) -> StoreResult<Vec<(u32, Vec<u8>)>> {
        let rows: Vec<(u32, Vec<u8>)> = self.with_conn(|conn| {
            let mut statement = conn.prepare("SELECT id, v FROM signed_prekeys ORDER BY id")?;
            let mapped = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            mapped.collect()
        })?;

        rows.into_iter()
            .map(|(id, envelope)| {
                let k = self.digest("signed_prekey", &id.to_le_bytes());
                self.open_value("signed_prekey", &k, &envelope)
                    .map(|record| (id, record))
            })
            .collect()
    }

    async fn remove_signed_prekey(&self, id: u32) -> StoreResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM signed_prekeys WHERE id = ?1", params![id])
                .map(|_| ())
        })
    }

    async fn put_sender_key(&self, address: &str, record: &[u8]) -> StoreResult<()> {
        self.kv_put(ns::SENDER_KEY, address.as_bytes(), record)
    }

    async fn get_sender_key(&self, address: &str) -> StoreResult<Option<Vec<u8>>> {
        self.kv_get(ns::SENDER_KEY, address.as_bytes())
    }

    async fn delete_sender_key(&self, address: &str) -> StoreResult<()> {
        self.kv_delete(ns::SENDER_KEY, address.as_bytes())
    }
}

#[async_trait]
impl AppSyncStore for EncryptedProtocolStore {
    async fn get_sync_key(&self, key_id: &[u8]) -> StoreResult<Option<AppStateSyncKey>> {
        let k = self.digest(ns::SYNC_KEY, key_id);
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v FROM sync_keys WHERE k = ?1",
                params![&k[..]],
                |row| row.get(0),
            )
            .optional()
        })?;
        match stored {
            Some(envelope) => {
                let raw = self.open_value(ns::SYNC_KEY, &k, &envelope)?;
                Ok(Some(serde_json::from_slice(&raw).map_err(ser_err)?))
            }
            None => Ok(None),
        }
    }

    async fn set_sync_key(&self, key_id: &[u8], key: AppStateSyncKey) -> StoreResult<()> {
        let k = self.digest(ns::SYNC_KEY, key_id);
        let encoded = serde_json::to_vec(&key).map_err(ser_err)?;
        let v = self.seal(ns::SYNC_KEY, &k, &encoded);
        let key_id_v = self.seal("sync_key_id", &k, key_id);
        let now = now_secs();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sync_keys (k, key_id_v, v, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v, updated_at = excluded.updated_at",
                params![&k[..], key_id_v, v, now],
            )
            .map(|_| ())
        })
    }

    async fn get_latest_sync_key_id(&self) -> StoreResult<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>, Vec<u8>)> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT k, key_id_v FROM sync_keys ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })?;
        match row {
            Some((k, key_id_v)) => Ok(Some(self.open_value("sync_key_id", &k, &key_id_v)?)),
            None => Ok(None),
        }
    }

    async fn get_version(&self, name: &str) -> StoreResult<HashState> {
        Ok(self
            .kv_get_json(ns::APP_VERSION, name.as_bytes())?
            .unwrap_or_default())
    }

    async fn set_version(&self, name: &str, state: HashState) -> StoreResult<()> {
        self.kv_put_json(ns::APP_VERSION, name.as_bytes(), &state)
    }

    async fn put_mutation_macs(
        &self,
        name: &str,
        version: u64,
        mutations: &[AppStateMutationMAC],
    ) -> StoreResult<()> {
        if mutations.is_empty() {
            return Ok(());
        }
        let name_k = self.digest(ns::MAC_NAME, name.as_bytes());
        let rows: Vec<([u8; 32], Vec<u8>)> = mutations
            .iter()
            .map(|mutation| {
                let index_k = self.digest(ns::MAC_INDEX, &mutation.index_mac);
                let v = self.seal(ns::MAC_INDEX, &index_k, &mutation.value_mac);
                (index_k, v)
            })
            .collect();

        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut statement = tx.prepare(
                    "INSERT INTO mutation_macs (name_k, index_k, v, version) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(name_k, index_k) DO UPDATE SET v = excluded.v, version = excluded.version",
                )?;
                for (index_k, v) in &rows {
                    statement.execute(params![&name_k[..], &index_k[..], v, version as i64])?;
                }
            }
            tx.commit()
        })
    }

    async fn get_mutation_mac(&self, name: &str, index_mac: &[u8]) -> StoreResult<Option<Vec<u8>>> {
        let name_k = self.digest(ns::MAC_NAME, name.as_bytes());
        let index_k = self.digest(ns::MAC_INDEX, index_mac);
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v FROM mutation_macs WHERE name_k = ?1 AND index_k = ?2",
                params![&name_k[..], &index_k[..]],
                |row| row.get(0),
            )
            .optional()
        })?;
        stored
            .map(|envelope| self.open_value(ns::MAC_INDEX, &index_k, &envelope))
            .transpose()
    }

    async fn delete_mutation_macs(&self, name: &str, index_macs: &[Vec<u8>]) -> StoreResult<()> {
        if index_macs.is_empty() {
            return Ok(());
        }
        let name_k = self.digest(ns::MAC_NAME, name.as_bytes());
        let digests: Vec<[u8; 32]> = index_macs
            .iter()
            .map(|mac| self.digest(ns::MAC_INDEX, mac))
            .collect();
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut statement =
                    tx.prepare("DELETE FROM mutation_macs WHERE name_k = ?1 AND index_k = ?2")?;
                for index_k in &digests {
                    statement.execute(params![&name_k[..], &index_k[..]])?;
                }
            }
            tx.commit()
        })
    }

    async fn clear_mutation_macs(&self, name: &str) -> StoreResult<()> {
        let name_k = self.digest(ns::MAC_NAME, name.as_bytes());
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM mutation_macs WHERE name_k = ?1",
                params![&name_k[..]],
            )
            .map(|_| ())
        })
    }
}

#[async_trait]
impl ProtocolStore for EncryptedProtocolStore {
    async fn get_sender_key_devices(&self, group_jid: &str) -> StoreResult<Vec<(String, bool)>> {
        let group_k = self.digest(ns::SKD_GROUP, group_jid.as_bytes());
        let rows: Vec<(Vec<u8>, Vec<u8>, i64)> = self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT device_k, device_v, has_key FROM sender_key_devices WHERE group_k = ?1",
            )?;
            let mapped = statement.query_map(params![&group_k[..]], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
            mapped.collect()
        })?;

        rows.into_iter()
            .map(|(device_k, device_v, has_key)| {
                let raw = self.open_value(ns::SKD_DEVICE, &device_k, &device_v)?;
                let jid = String::from_utf8(raw)
                    .map_err(|_| StoreError::Validation("device JID is not UTF-8".into()))?;
                Ok((jid, has_key != 0))
            })
            .collect()
    }

    async fn set_sender_key_status(
        &self,
        group_jid: &str,
        entries: &[(&str, bool)],
    ) -> StoreResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let group_k = self.digest(ns::SKD_GROUP, group_jid.as_bytes());
        let rows: Vec<([u8; 32], Vec<u8>, i64)> = entries
            .iter()
            .map(|(device, has_key)| {
                let device_k = self.digest(ns::SKD_DEVICE, device.as_bytes());
                let device_v = self.seal(ns::SKD_DEVICE, &device_k, device.as_bytes());
                (device_k, device_v, *has_key as i64)
            })
            .collect();

        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut statement = tx.prepare(
                    "INSERT INTO sender_key_devices (group_k, device_k, device_v, has_key)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(group_k, device_k) DO UPDATE SET has_key = excluded.has_key",
                )?;
                for (device_k, device_v, has_key) in &rows {
                    statement.execute(params![&group_k[..], &device_k[..], device_v, has_key])?;
                }
            }
            tx.commit()
        })
    }

    async fn clear_sender_key_devices(&self, group_jid: &str) -> StoreResult<()> {
        let group_k = self.digest(ns::SKD_GROUP, group_jid.as_bytes());
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM sender_key_devices WHERE group_k = ?1",
                params![&group_k[..]],
            )
            .map(|_| ())
        })
    }

    async fn delete_sender_key_device_rows(&self, device_jids: &[&str]) -> StoreResult<()> {
        if device_jids.is_empty() {
            return Ok(());
        }
        let digests: Vec<[u8; 32]> = device_jids
            .iter()
            .map(|jid| self.digest(ns::SKD_DEVICE, jid.as_bytes()))
            .collect();
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut statement =
                    tx.prepare("DELETE FROM sender_key_devices WHERE device_k = ?1")?;
                for device_k in &digests {
                    statement.execute(params![&device_k[..]])?;
                }
            }
            tx.commit()
        })
    }

    async fn clear_all_sender_key_devices(&self) -> StoreResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM sender_key_devices", [])
                .map(|_| ())
        })
    }

    async fn get_lid_mapping(&self, lid: &str) -> StoreResult<Option<LidPnMappingEntry>> {
        let lid_k = self.digest(ns::LID, lid.as_bytes());
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v FROM lid_map WHERE lid_k = ?1",
                params![&lid_k[..]],
                |row| row.get(0),
            )
            .optional()
        })?;
        match stored {
            Some(envelope) => {
                let raw = self.open_value(ns::LID, &lid_k, &envelope)?;
                Ok(Some(serde_json::from_slice(&raw).map_err(ser_err)?))
            }
            None => Ok(None),
        }
    }

    async fn get_pn_mapping(&self, phone: &str) -> StoreResult<Option<LidPnMappingEntry>> {
        let pn_k = self.digest(ns::PN, phone.as_bytes());
        let row: Option<(Vec<u8>, Vec<u8>)> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT lid_k, v FROM lid_map WHERE pn_k = ?1 ORDER BY updated_at DESC LIMIT 1",
                params![&pn_k[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })?;
        match row {
            Some((lid_k, envelope)) => {
                let raw = self.open_value(ns::LID, &lid_k, &envelope)?;
                Ok(Some(serde_json::from_slice(&raw).map_err(ser_err)?))
            }
            None => Ok(None),
        }
    }

    async fn put_lid_mapping(&self, entry: &LidPnMappingEntry) -> StoreResult<()> {
        self.put_lid_mappings(std::slice::from_ref(entry)).await
    }

    async fn put_lid_mappings(&self, entries: &[LidPnMappingEntry]) -> StoreResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut rows = Vec::with_capacity(entries.len());
        for entry in entries {
            let lid_k = self.digest(ns::LID, entry.lid.as_bytes());
            let pn_k = self.digest(ns::PN, entry.phone_number.as_bytes());
            let encoded = serde_json::to_vec(entry).map_err(ser_err)?;
            let v = self.seal(ns::LID, &lid_k, &encoded);
            rows.push((lid_k, pn_k, v, entry.updated_at));
        }

        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut statement = tx.prepare(
                    "INSERT INTO lid_map (lid_k, pn_k, v, updated_at) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(lid_k) DO UPDATE SET
                        pn_k = excluded.pn_k,
                        v = excluded.v,
                        updated_at = excluded.updated_at",
                )?;
                for (lid_k, pn_k, v, updated_at) in &rows {
                    statement.execute(params![&lid_k[..], &pn_k[..], v, updated_at])?;
                }
            }
            tx.commit()
        })
    }

    async fn get_all_lid_mappings(&self) -> StoreResult<Vec<LidPnMappingEntry>> {
        let rows: Vec<(Vec<u8>, Vec<u8>)> = self.with_conn(|conn| {
            let mut statement = conn.prepare("SELECT lid_k, v FROM lid_map")?;
            let mapped = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            mapped.collect()
        })?;

        rows.into_iter()
            .map(|(lid_k, envelope)| {
                let raw = self.open_value(ns::LID, &lid_k, &envelope)?;
                serde_json::from_slice(&raw).map_err(ser_err)
            })
            .collect()
    }

    async fn save_base_key(
        &self,
        address: &str,
        message_id: &str,
        base_key: &[u8],
    ) -> StoreResult<()> {
        let key = base_key_id(address, message_id);
        self.kv_put(ns::BASE_KEY, key.as_bytes(), base_key)
    }

    async fn has_same_base_key(
        &self,
        address: &str,
        message_id: &str,
        current_base_key: &[u8],
    ) -> StoreResult<bool> {
        let key = base_key_id(address, message_id);
        Ok(self
            .kv_get(ns::BASE_KEY, key.as_bytes())?
            .is_some_and(|stored| stored == current_base_key))
    }

    async fn delete_base_key(&self, address: &str, message_id: &str) -> StoreResult<()> {
        let key = base_key_id(address, message_id);
        self.kv_delete(ns::BASE_KEY, key.as_bytes())
    }

    async fn update_device_list(&self, record: DeviceListRecord) -> StoreResult<()> {
        let user = record.user.clone();
        self.kv_put_json(ns::DEVICE_LIST, user.as_bytes(), &record)
    }

    async fn get_devices(&self, user: &str) -> StoreResult<Option<DeviceListRecord>> {
        self.kv_get_json(ns::DEVICE_LIST, user.as_bytes())
    }

    async fn delete_devices(&self, user: &str) -> StoreResult<()> {
        self.kv_delete(ns::DEVICE_LIST, user.as_bytes())
    }

    async fn get_tc_token(&self, jid: &str) -> StoreResult<Option<TcTokenEntry>> {
        let k = self.digest(ns::TC_TOKEN, jid.as_bytes());
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v FROM tc_tokens WHERE k = ?1",
                params![&k[..]],
                |row| row.get(0),
            )
            .optional()
        })?;
        match stored {
            Some(envelope) => {
                let raw = self.open_value(ns::TC_TOKEN, &k, &envelope)?;
                Ok(Some(serde_json::from_slice(&raw).map_err(ser_err)?))
            }
            None => Ok(None),
        }
    }

    async fn put_tc_token(&self, jid: &str, entry: &TcTokenEntry) -> StoreResult<()> {
        let k = self.digest(ns::TC_TOKEN, jid.as_bytes());
        let encoded = serde_json::to_vec(entry).map_err(ser_err)?;
        let v = self.seal(ns::TC_TOKEN, &k, &encoded);
        let jid_v = self.seal("tc_token_jid", &k, jid.as_bytes());
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tc_tokens (k, jid_v, v, token_ts, sender_ts) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(k) DO UPDATE SET
                    v = excluded.v,
                    token_ts = excluded.token_ts,
                    sender_ts = excluded.sender_ts",
                params![&k[..], jid_v, v, entry.token_timestamp, entry.sender_timestamp],
            )
            .map(|_| ())
        })
    }

    async fn delete_tc_token(&self, jid: &str) -> StoreResult<()> {
        let k = self.digest(ns::TC_TOKEN, jid.as_bytes());
        self.with_conn(|conn| {
            conn.execute("DELETE FROM tc_tokens WHERE k = ?1", params![&k[..]])
                .map(|_| ())
        })
    }

    async fn get_all_tc_token_jids(&self) -> StoreResult<Vec<String>> {
        let rows: Vec<(Vec<u8>, Vec<u8>)> = self.with_conn(|conn| {
            let mut statement = conn.prepare("SELECT k, jid_v FROM tc_tokens")?;
            let mapped = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            mapped.collect()
        })?;

        rows.into_iter()
            .map(|(k, jid_v)| {
                let raw = self.open_value("tc_token_jid", &k, &jid_v)?;
                String::from_utf8(raw)
                    .map_err(|_| StoreError::Validation("tc token JID is not UTF-8".into()))
            })
            .collect()
    }

    async fn delete_expired_tc_tokens(
        &self,
        token_cutoff: i64,
        sender_cutoff: i64,
    ) -> StoreResult<u32> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM tc_tokens
                 WHERE token_ts < ?1
                   AND (sender_ts IS NULL OR sender_ts < ?2)",
                params![token_cutoff, sender_cutoff],
            )
        })
        .map(|deleted| deleted as u32)
    }

    async fn store_sent_message(
        &self,
        chat_jid: &str,
        message_id: &str,
        payload: &[u8],
    ) -> StoreResult<()> {
        let k = self.key.digest_parts(
            ns::SENT_MESSAGE,
            &[chat_jid.as_bytes(), message_id.as_bytes()],
        );
        let v = self.seal(ns::SENT_MESSAGE, &k, payload);
        let now = now_secs();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sent_messages (k, v, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v, created_at = excluded.created_at",
                params![&k[..], v, now],
            )
            .map(|_| ())
        })
    }

    async fn take_sent_message(
        &self,
        chat_jid: &str,
        message_id: &str,
    ) -> StoreResult<Option<Vec<u8>>> {
        let k = self.key.digest_parts(
            ns::SENT_MESSAGE,
            &[chat_jid.as_bytes(), message_id.as_bytes()],
        );
        // Atomic take: a retry receipt must never yield the same payload twice.
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let found: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT v FROM sent_messages WHERE k = ?1",
                    params![&k[..]],
                    |row| row.get(0),
                )
                .optional()?;
            if found.is_some() {
                tx.execute("DELETE FROM sent_messages WHERE k = ?1", params![&k[..]])?;
            }
            tx.commit()?;
            Ok(found)
        })?;
        stored
            .map(|envelope| self.open_value(ns::SENT_MESSAGE, &k, &envelope))
            .transpose()
    }

    async fn delete_expired_sent_messages(&self, cutoff_timestamp: i64) -> StoreResult<u32> {
        // Plan 023 caps retry retention at 24 hours. A caller asking to keep
        // rows longer is clamped; a caller asking for less is honoured.
        let hard_cutoff = now_secs() - MAX_RETRY_RETENTION_SECONDS;
        let cutoff = cutoff_timestamp.max(hard_cutoff);
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM sent_messages WHERE created_at <= ?1",
                params![cutoff],
            )
        })
        .map(|deleted| deleted as u32)
    }
}

#[async_trait]
impl MsgSecretStore for EncryptedProtocolStore {
    async fn put_msg_secrets(&self, entries: Vec<MsgSecretEntry>) -> StoreResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let mut rows = Vec::with_capacity(entries.len());
        for entry in &entries {
            let k = self.key.digest_parts(
                ns::MSG_SECRET,
                &[
                    entry.chat.as_bytes(),
                    entry.sender.as_bytes(),
                    entry.msg_id.as_bytes(),
                ],
            );
            let v = self.seal(ns::MSG_SECRET, &k, &entry.secret);
            rows.push((k, v, entry.expires_at, entry.message_ts));
        }

        let written = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut written = 0usize;
            {
                let mut statement = tx.prepare(
                    "INSERT INTO msg_secrets (k, v, expires_at, message_ts) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(k) DO UPDATE SET
                        v = excluded.v,
                        expires_at = ?5,
                        message_ts = ?6",
                )?;
                for (k, v, expires_at, message_ts) in &rows {
                    // Merge rules live in wacore so a redelivery never shortens
                    // a retention window or clobbers a known parent timestamp.
                    let existing: Option<(i64, i64)> = tx
                        .query_row(
                            "SELECT expires_at, message_ts FROM msg_secrets WHERE k = ?1",
                            params![&k[..]],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .optional()?;
                    let (merged_expiry, merged_ts) = match existing {
                        Some((old_expiry, old_ts)) => (
                            merge_msg_secret_expiry(old_expiry, *expires_at),
                            merge_msg_secret_message_ts(old_ts, *message_ts),
                        ),
                        None => (*expires_at, *message_ts),
                    };
                    written += statement.execute(params![
                        &k[..],
                        v,
                        expires_at,
                        message_ts,
                        merged_expiry,
                        merged_ts
                    ])?;
                }
            }
            tx.commit()?;
            Ok(written)
        })?;
        Ok(written)
    }

    async fn get_msg_secret(
        &self,
        chat: &str,
        sender: &str,
        msg_id: &str,
    ) -> StoreResult<Option<Vec<u8>>> {
        Ok(self
            .get_msg_secret_with_ts(chat, sender, msg_id)
            .await?
            .map(|(secret, _)| secret))
    }

    async fn get_msg_secret_with_ts(
        &self,
        chat: &str,
        sender: &str,
        msg_id: &str,
    ) -> StoreResult<Option<(Vec<u8>, i64)>> {
        let k = self.key.digest_parts(
            ns::MSG_SECRET,
            &[chat.as_bytes(), sender.as_bytes(), msg_id.as_bytes()],
        );
        let row: Option<(Vec<u8>, i64)> = self.with_conn(|conn| {
            conn.query_row(
                "SELECT v, message_ts FROM msg_secrets WHERE k = ?1",
                params![&k[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })?;
        match row {
            Some((envelope, message_ts)) => {
                let secret = self.open_value(ns::MSG_SECRET, &k, &envelope)?;
                Ok(Some((secret, message_ts)))
            }
            None => Ok(None),
        }
    }

    async fn delete_expired_msg_secrets(&self, cutoff_timestamp: i64) -> StoreResult<u32> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM msg_secrets WHERE expires_at != 0 AND expires_at <= ?1",
                params![cutoff_timestamp],
            )
        })
        .map(|deleted| deleted as u32)
    }
}

#[async_trait]
impl DeviceStore for EncryptedProtocolStore {
    async fn save(&self, device: &Device) -> StoreResult<()> {
        let encoded = serde_json::to_vec(device).map_err(ser_err)?;
        let k = self.digest("device", b"self");
        let v = self.seal("device", &k, &encoded);
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO device (id, v) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET v = excluded.v",
                params![v],
            )
            .map(|_| ())
        })
    }

    async fn load(&self) -> StoreResult<Option<Device>> {
        let stored: Option<Vec<u8>> = self.with_conn(|conn| {
            conn.query_row("SELECT v FROM device WHERE id = 1", [], |row| row.get(0))
                .optional()
        })?;
        match stored {
            Some(envelope) => {
                let k = self.digest("device", b"self");
                let raw = self.open_value("device", &k, &envelope)?;
                Ok(Some(serde_json::from_slice(&raw).map_err(ser_err)?))
            }
            None => Ok(None),
        }
    }

    async fn exists(&self) -> StoreResult<bool> {
        self.with_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM device WHERE id = 1", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .map(|count| count > 0)
    }

    async fn create(&self) -> StoreResult<i32> {
        // Exactly one linked device per Alfred installation (Plan 023).
        Ok(1)
    }
}

/// Composite lookup key for the retry base-key table.
fn base_key_id(address: &str, message_id: &str) -> String {
    format!("{address}\u{0}{message_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const ADDRESS: &str = "34600123456.0:17@s.whatsapp.net";
    const CHAT: &str = "34600123456@s.whatsapp.net";

    fn store() -> EncryptedProtocolStore {
        EncryptedProtocolStore::open_in_memory(StoreKey::generate()).unwrap()
    }

    #[tokio::test]
    async fn identities_roundtrip_and_delete() {
        let store = store();
        assert!(store.load_identity(ADDRESS).await.unwrap().is_none());

        store.put_identity(ADDRESS, [9u8; 32]).await.unwrap();
        assert_eq!(store.load_identity(ADDRESS).await.unwrap(), Some([9u8; 32]));

        store.delete_identity(ADDRESS).await.unwrap();
        assert!(store.load_identity(ADDRESS).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn sessions_roundtrip_and_report_existence() {
        let store = store();
        assert!(!store.has_session(ADDRESS).await.unwrap());

        store.put_session(ADDRESS, b"session-bytes").await.unwrap();
        assert!(store.has_session(ADDRESS).await.unwrap());
        assert_eq!(
            store.get_session(ADDRESS).await.unwrap().unwrap(),
            Bytes::from_static(b"session-bytes")
        );

        store.delete_session(ADDRESS).await.unwrap();
        assert!(!store.has_session(ADDRESS).await.unwrap());
    }

    #[tokio::test]
    async fn prekeys_track_max_id_and_upload_state() {
        let store = store();
        assert_eq!(store.get_max_prekey_id().await.unwrap(), 0);

        store.store_prekey(1, b"one", false).await.unwrap();
        store.store_prekey(7, b"seven", false).await.unwrap();
        assert_eq!(store.get_max_prekey_id().await.unwrap(), 7);
        assert_eq!(store.load_prekey(7).await.unwrap().unwrap(), &b"seven"[..]);

        store.remove_prekey(7).await.unwrap();
        // UPDATE semantics: a consumed key must not be resurrected.
        store.mark_prekeys_uploaded(&[1, 7]).await.unwrap();
        assert!(store.load_prekey(7).await.unwrap().is_none());
        assert_eq!(store.get_max_prekey_id().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn signed_prekeys_load_all_in_id_order() {
        let store = store();
        store.store_signed_prekey(2, b"two").await.unwrap();
        store.store_signed_prekey(1, b"one").await.unwrap();

        let all = store.load_all_signed_prekeys().await.unwrap();
        assert_eq!(
            all,
            vec![(1, b"one".to_vec()), (2, b"two".to_vec())],
            "signed pre-keys must come back ordered by id"
        );

        store.remove_signed_prekey(1).await.unwrap();
        assert!(store.load_signed_prekey(1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn lid_mappings_resolve_from_both_directions() {
        let store = store();
        let entry = LidPnMappingEntry {
            lid: "237756605284433".into(),
            phone_number: "34600123456".into(),
            created_at: 100,
            updated_at: 100,
            learning_source: "usync".into(),
        };
        store.put_lid_mapping(&entry).await.unwrap();

        assert_eq!(
            store
                .get_lid_mapping("237756605284433")
                .await
                .unwrap()
                .unwrap()
                .phone_number,
            "34600123456"
        );
        assert_eq!(
            store
                .get_pn_mapping("34600123456")
                .await
                .unwrap()
                .unwrap()
                .lid,
            "237756605284433"
        );
        assert_eq!(store.get_all_lid_mappings().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sender_key_devices_round_trip_per_group() {
        let store = store();
        let group = "123-456@g.us";
        store
            .set_sender_key_status(
                group,
                &[("a@s.whatsapp.net", true), ("b@s.whatsapp.net", false)],
            )
            .await
            .unwrap();

        let mut devices = store.get_sender_key_devices(group).await.unwrap();
        devices.sort();
        assert_eq!(
            devices,
            vec![
                ("a@s.whatsapp.net".to_string(), true),
                ("b@s.whatsapp.net".to_string(), false),
            ]
        );

        store
            .delete_sender_key_device_rows(&["a@s.whatsapp.net"])
            .await
            .unwrap();
        assert_eq!(store.get_sender_key_devices(group).await.unwrap().len(), 1);

        store.clear_all_sender_key_devices().await.unwrap();
        assert!(store
            .get_sender_key_devices(group)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn base_keys_compare_only_on_exact_match() {
        let store = store();
        store
            .save_base_key(ADDRESS, "msg-1", b"base")
            .await
            .unwrap();

        assert!(store
            .has_same_base_key(ADDRESS, "msg-1", b"base")
            .await
            .unwrap());
        assert!(!store
            .has_same_base_key(ADDRESS, "msg-1", b"other")
            .await
            .unwrap());
        // The composite key must not collide across message ids.
        assert!(!store
            .has_same_base_key(ADDRESS, "msg-2", b"base")
            .await
            .unwrap());

        store.delete_base_key(ADDRESS, "msg-1").await.unwrap();
        assert!(!store
            .has_same_base_key(ADDRESS, "msg-1", b"base")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn sent_message_take_is_single_use() {
        let store = store();
        store
            .store_sent_message(CHAT, "3EB0", b"retry-payload")
            .await
            .unwrap();

        assert_eq!(
            store
                .take_sent_message(CHAT, "3EB0")
                .await
                .unwrap()
                .unwrap(),
            b"retry-payload".to_vec()
        );
        assert!(
            store
                .take_sent_message(CHAT, "3EB0")
                .await
                .unwrap()
                .is_none(),
            "a consumed retry payload must never be handed out twice"
        );
    }

    #[tokio::test]
    async fn retry_retention_is_clamped_to_24_hours() {
        let store = store();
        store.store_sent_message(CHAT, "fresh", b"x").await.unwrap();

        // Backdate one row well past the ceiling.
        store
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE sent_messages SET created_at = ?1",
                    params![now_secs() - MAX_RETRY_RETENTION_SECONDS - 60],
                )
                .map(|_| ())
            })
            .unwrap();

        // A caller asking to keep everything (cutoff far in the past) is still
        // clamped to the 24-hour ceiling.
        assert_eq!(store.delete_expired_sent_messages(0).await.unwrap(), 1);
        assert!(store
            .take_sent_message(CHAT, "fresh")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn msg_secret_expiry_never_shortens() {
        let store = store();
        let entry = |expires_at: i64, message_ts: i64| MsgSecretEntry {
            chat: Arc::from(CHAT),
            sender: Arc::from(CHAT),
            msg_id: Arc::from("m1"),
            secret: [3u8; 32],
            expires_at,
            message_ts,
        };

        store.put_msg_secrets(vec![entry(5_000, 90)]).await.unwrap();
        // A redelivery with an earlier deadline must not shrink the window.
        store.put_msg_secrets(vec![entry(1_000, 0)]).await.unwrap();

        assert_eq!(store.delete_expired_msg_secrets(2_000).await.unwrap(), 0);
        let (secret, message_ts) = store
            .get_msg_secret_with_ts(CHAT, CHAT, "m1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(secret, [3u8; 32].to_vec());
        assert_eq!(
            message_ts, 90,
            "a zero parent timestamp must not clobber a known one"
        );

        assert_eq!(store.delete_expired_msg_secrets(6_000).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn msg_secrets_with_no_expiry_are_never_purged() {
        let store = store();
        store
            .put_msg_secrets(vec![MsgSecretEntry {
                chat: Arc::from(CHAT),
                sender: Arc::from(CHAT),
                msg_id: Arc::from("forever"),
                secret: [1u8; 32],
                expires_at: 0,
                message_ts: 0,
            }])
            .await
            .unwrap();

        assert_eq!(store.delete_expired_msg_secrets(i64::MAX).await.unwrap(), 0);
        assert!(store
            .get_msg_secret(CHAT, CHAT, "forever")
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn tc_tokens_expire_only_when_both_buckets_are_stale() {
        let store = store();
        let jid = "34600123456@lid";
        store
            .put_tc_token(
                jid,
                &TcTokenEntry {
                    token: b"tok".to_vec(),
                    token_timestamp: 100,
                    sender_timestamp: Some(900),
                },
            )
            .await
            .unwrap();

        // Received token is stale but the sender bucket is fresh: keep the row.
        assert_eq!(store.delete_expired_tc_tokens(500, 500).await.unwrap(), 0);
        assert_eq!(store.get_all_tc_token_jids().await.unwrap(), vec![jid]);

        assert_eq!(store.delete_expired_tc_tokens(500, 1_000).await.unwrap(), 1);
        assert!(store.get_tc_token(jid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn app_state_versions_and_macs_round_trip() {
        let store = store();
        assert_eq!(store.get_version("regular").await.unwrap().version, 0);

        let state = HashState {
            version: 7,
            hash: [1u8; 128],
            index_value_map: HashMap::new(),
            ..Default::default()
        };
        store.set_version("regular", state).await.unwrap();
        assert_eq!(store.get_version("regular").await.unwrap().version, 7);

        store
            .put_mutation_macs(
                "regular",
                7,
                &[AppStateMutationMAC {
                    index_mac: vec![1, 2, 3],
                    value_mac: vec![4, 5, 6],
                }],
            )
            .await
            .unwrap();
        assert_eq!(
            store.get_mutation_mac("regular", &[1, 2, 3]).await.unwrap(),
            Some(vec![4, 5, 6])
        );
        // Collections are independent.
        assert!(store
            .get_mutation_mac("critical", &[1, 2, 3])
            .await
            .unwrap()
            .is_none());

        store.clear_mutation_macs("regular").await.unwrap();
        assert!(store
            .get_mutation_mac("regular", &[1, 2, 3])
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn sync_keys_report_the_most_recent_id() {
        let store = store();
        assert!(store.get_latest_sync_key_id().await.unwrap().is_none());

        store
            .set_sync_key(b"key-a", AppStateSyncKey::default())
            .await
            .unwrap();
        assert_eq!(
            store.get_latest_sync_key_id().await.unwrap(),
            Some(b"key-a".to_vec())
        );
        assert!(store.get_sync_key(b"key-a").await.unwrap().is_some());
        assert!(store.get_sync_key(b"missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn device_lists_round_trip() {
        let store = store();
        let record = DeviceListRecord {
            user: "34600123456".into(),
            devices: Vec::new(),
            timestamp: 42,
            phash: None,
            raw_id: Some(3),
        };
        store.update_device_list(record).await.unwrap();

        let loaded = store.get_devices("34600123456").await.unwrap().unwrap();
        assert_eq!(loaded.timestamp, 42);
        assert_eq!(loaded.raw_id, Some(3));

        store.delete_devices("34600123456").await.unwrap();
        assert!(store.get_devices("34600123456").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn inbound_buffer_fails_closed() {
        let store = store();
        // Plan 023 stores no inbound content. The trait defaults must surface an
        // error rather than silently degrading to at-most-once delivery.
        assert!(store
            .store_pending_inbound("c", "s", "i", b"body")
            .await
            .is_err());
        assert!(store.get_pending_inbound("c", "s", "i").await.is_err());
        // The keepalive sweep calls this unconditionally, so it must not error.
        assert_eq!(
            store
                .delete_expired_pending_inbound(now_secs())
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn group_metadata_is_never_persisted() {
        let store = store();
        store.put_group_metadata("123@g.us", b"blob").await.unwrap();
        assert!(
            store
                .get_group_metadata("123@g.us")
                .await
                .unwrap()
                .is_none(),
            "Plan 023 stores no group metadata"
        );
    }

    // --- at-rest guarantees -------------------------------------------------

    #[tokio::test]
    async fn a_wrong_key_cannot_read_an_existing_store() {
        let dir = tempdir();
        let path = dir.join("whatsapp.db");

        let key = StoreKey::generate();
        let encoded = key.expose_base64();
        {
            let store = EncryptedProtocolStore::open(&path, key).unwrap();
            store.put_session(ADDRESS, b"secret-session").await.unwrap();
        }

        // Right key: readable.
        {
            let store =
                EncryptedProtocolStore::open(&path, StoreKey::from_base64(&encoded).unwrap())
                    .unwrap();
            assert!(store.get_session(ADDRESS).await.unwrap().is_some());
        }

        // Wrong key: the row is not even addressable, and forcing a read fails.
        {
            let store = EncryptedProtocolStore::open(&path, StoreKey::generate()).unwrap();
            assert!(store.get_session(ADDRESS).await.unwrap().is_none());
        }

        cleanup(&dir);
    }

    #[tokio::test]
    async fn the_database_file_holds_no_plaintext_identity() {
        let dir = tempdir();
        let path = dir.join("whatsapp.db");
        let sentinel_jid = "34600123456@s.whatsapp.net";
        let sentinel_body = b"SENTINEL-RETRY-BODY";

        {
            let store = EncryptedProtocolStore::open(&path, StoreKey::generate()).unwrap();
            store.put_session(sentinel_jid, b"session").await.unwrap();
            store.put_identity(sentinel_jid, [4u8; 32]).await.unwrap();
            store
                .store_sent_message(sentinel_jid, "3EB0", sentinel_body)
                .await
                .unwrap();
            store
                .put_lid_mapping(&LidPnMappingEntry {
                    lid: "237756605284433".into(),
                    phone_number: "34600123456".into(),
                    created_at: 1,
                    updated_at: 1,
                    learning_source: "usync".into(),
                })
                .await
                .unwrap();
        }

        // Scan the database and every sidecar SQLite may have left behind.
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            for needle in [
                sentinel_jid.as_bytes(),
                &sentinel_body[..],
                b"34600123456",
                b"237756605284433",
            ] {
                assert!(
                    !bytes.windows(needle.len()).any(|w| w == needle),
                    "{} leaked {:?} in plaintext",
                    candidate.display(),
                    String::from_utf8_lossy(needle)
                );
            }
        }

        cleanup(&dir);
    }

    #[tokio::test]
    async fn state_survives_a_reopen() {
        let dir = tempdir();
        let path = dir.join("whatsapp.db");
        let key = StoreKey::generate();
        let encoded = key.expose_base64();

        {
            let store = EncryptedProtocolStore::open(&path, key).unwrap();
            store.store_prekey(11, b"record", true).await.unwrap();
        }
        {
            let store =
                EncryptedProtocolStore::open(&path, StoreKey::from_base64(&encoded).unwrap())
                    .unwrap();
            assert_eq!(
                store.load_prekey(11).await.unwrap().unwrap(),
                &b"record"[..]
            );
        }

        cleanup(&dir);
    }

    #[test]
    fn delete_files_removes_every_sidecar() {
        let dir = tempdir();
        let path = dir.join("whatsapp.db");
        for suffix in ["", "-wal", "-shm"] {
            std::fs::write(format!("{}{suffix}", path.display()), b"x").unwrap();
        }

        assert_eq!(EncryptedProtocolStore::delete_files(&path).unwrap(), 3);
        // Idempotent: a second disconnect attempt must not error.
        assert_eq!(EncryptedProtocolStore::delete_files(&path).unwrap(), 0);

        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_database_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir();
        let path = dir.join("whatsapp.db");
        EncryptedProtocolStore::open(&path, StoreKey::generate()).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "protocol store must not be group- or world-readable"
        );

        cleanup(&dir);
    }

    fn tempdir() -> PathBuf {
        let unique = format!(
            "alfred-whatsapp-store-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let dir = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
