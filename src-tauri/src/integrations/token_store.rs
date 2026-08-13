use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Mutex;
use thiserror::Error;
use zeroize::Zeroize;

pub const TOKEN_STORE_SERVICE: &str = "com.alfred.connected-apps";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEnvelope {
    pub version: u8,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub provider_fields: BTreeMap<String, String>,
}

impl CredentialEnvelope {
    pub const CURRENT_VERSION: u8 = 1;

    pub fn new(access_token: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            access_token,
            refresh_token: None,
            expires_at: None,
            provider_fields: BTreeMap::new(),
        }
    }
}

impl fmt::Debug for CredentialEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialEnvelope")
            .field("version", &self.version)
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_at", &self.expires_at)
            .field("provider_fields", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CredentialEnvelope {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        for value in self.provider_fields.values_mut() {
            value.zeroize();
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum TokenStoreError {
    #[error("credential was not found")]
    Missing,
    #[error("credential storage is locked or unavailable")]
    Locked,
    #[error("credential payload is invalid")]
    Invalid,
    #[error("credential operation failed")]
    Failed,
}

pub trait TokenStore: Send + Sync {
    fn put(
        &self,
        credential_ref: &str,
        credential: &CredentialEnvelope,
    ) -> Result<(), TokenStoreError>;
    fn get(&self, credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError>;
    fn delete(&self, credential_ref: &str) -> Result<(), TokenStoreError>;
}

#[derive(Debug, Default)]
pub struct OsTokenStore;

impl OsTokenStore {
    fn entry(credential_ref: &str) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(TOKEN_STORE_SERVICE, credential_ref).map_err(map_keyring_error)
    }
}

impl TokenStore for OsTokenStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &CredentialEnvelope,
    ) -> Result<(), TokenStoreError> {
        let mut payload = serde_json::to_vec(credential).map_err(|_| TokenStoreError::Invalid)?;
        let result = match Self::entry(credential_ref) {
            Ok(entry) => entry.set_secret(&payload).map_err(map_keyring_error),
            Err(error) => Err(error),
        };
        payload.zeroize();
        result
    }

    fn get(&self, credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError> {
        let mut payload = Self::entry(credential_ref)?
            .get_secret()
            .map_err(map_keyring_error)?;
        let result = serde_json::from_slice::<CredentialEnvelope>(&payload)
            .map_err(|_| TokenStoreError::Invalid);
        payload.zeroize();
        let credential = result?;
        if credential.version != CredentialEnvelope::CURRENT_VERSION {
            return Err(TokenStoreError::Invalid);
        }
        Ok(credential)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), TokenStoreError> {
        Self::entry(credential_ref)?
            .delete_credential()
            .map_err(map_keyring_error)
    }
}

fn map_keyring_error(error: keyring::Error) -> TokenStoreError {
    match error {
        keyring::Error::NoEntry => TokenStoreError::Missing,
        keyring::Error::NoStorageAccess(_) | keyring::Error::NoDefaultStore => {
            TokenStoreError::Locked
        }
        _ => TokenStoreError::Failed,
    }
}

#[derive(Debug, Default)]
pub struct InMemoryTokenStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

impl TokenStore for InMemoryTokenStore {
    fn put(
        &self,
        credential_ref: &str,
        credential: &CredentialEnvelope,
    ) -> Result<(), TokenStoreError> {
        let payload = serde_json::to_vec(credential).map_err(|_| TokenStoreError::Invalid)?;
        let mut entries = self.entries.lock().map_err(|_| TokenStoreError::Failed)?;
        if let Some(mut previous) = entries.insert(credential_ref.to_owned(), payload) {
            previous.zeroize();
        }
        Ok(())
    }

    fn get(&self, credential_ref: &str) -> Result<CredentialEnvelope, TokenStoreError> {
        let entries = self.entries.lock().map_err(|_| TokenStoreError::Failed)?;
        let payload = entries
            .get(credential_ref)
            .ok_or(TokenStoreError::Missing)?;
        serde_json::from_slice(payload).map_err(|_| TokenStoreError::Invalid)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), TokenStoreError> {
        let mut removed = self
            .entries
            .lock()
            .map_err(|_| TokenStoreError::Failed)?
            .remove(credential_ref);
        if let Some(payload) = removed.as_mut() {
            payload.zeroize();
        } else {
            return Err(TokenStoreError::Missing);
        }
        Ok(())
    }
}

impl Drop for InMemoryTokenStore {
    fn drop(&mut self) {
        if let Ok(entries) = self.entries.get_mut() {
            for payload in entries.values_mut() {
                payload.zeroize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(value: &str) -> CredentialEnvelope {
        let mut envelope = CredentialEnvelope::new(value.into());
        envelope.refresh_token = Some(format!("refresh-{value}"));
        envelope
    }

    #[test]
    fn fake_store_round_trip_overwrite_delete_and_missing() {
        let store = InMemoryTokenStore::default();
        store.put("opaque", &credential("first")).expect("put");
        assert_eq!(store.get("opaque").expect("get").access_token, "first");

        store
            .put("opaque", &credential("second"))
            .expect("overwrite");
        assert_eq!(store.get("opaque").expect("get").access_token, "second");

        store.delete("opaque").expect("delete");
        assert_eq!(store.get("opaque").unwrap_err(), TokenStoreError::Missing);
        assert_eq!(
            store.delete("opaque").unwrap_err(),
            TokenStoreError::Missing
        );
    }

    #[test]
    fn debug_output_is_redacted() {
        let fixture = credential("access-secret-fixture");
        let output = format!("{fixture:?}");
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("access-secret-fixture"));
        assert!(!output.contains("refresh-access-secret-fixture"));
    }
}
