use super::models::{AppConnection, ConnectionStatus};
use super::token_store::{CredentialEnvelope, TokenStore, TokenStoreError};
use crate::db::Db;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

pub type RefreshFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CredentialEnvelope, ProviderRefreshError>> + Send + 'a>>;

pub trait RefreshHandler: Send + Sync {
    fn needs_refresh(&self, connection: &AppConnection, now: DateTime<Utc>) -> bool;
    fn refresh<'a>(
        &'a self,
        connection: &'a AppConnection,
        credential: CredentialEnvelope,
    ) -> RefreshFuture<'a>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshFailureKind {
    Retryable,
    Terminal,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("credential refresh failed ({code})")]
pub struct ProviderRefreshError {
    code: String,
    kind: RefreshFailureKind,
}

impl ProviderRefreshError {
    pub fn retryable(code: &str) -> Self {
        Self {
            code: stable_error_code(code),
            kind: RefreshFailureKind::Retryable,
        }
    }

    pub fn terminal(code: &str) -> Self {
        Self {
            code: stable_error_code(code),
            kind: RefreshFailureKind::Terminal,
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn kind(&self) -> RefreshFailureKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    Refreshed,
    NotNeeded,
}

#[derive(Debug, Error)]
pub enum RefreshServiceError {
    #[error("connection was not found")]
    NotFound,
    #[error("connection has been revoked")]
    Revoked,
    #[error("provider has no refresh handler")]
    NotRegistered,
    #[error("credential storage error: {0}")]
    TokenStore(#[from] TokenStoreError),
    #[error("provider refresh error: {0}")]
    Provider(#[from] ProviderRefreshError),
    #[error("connection metadata operation failed")]
    Database,
    #[error("refresh service lock failed")]
    Lock,
}

pub struct RefreshService {
    token_store: Arc<dyn TokenStore>,
    handlers: RwLock<HashMap<String, Arc<dyn RefreshHandler>>>,
    connection_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl RefreshService {
    pub fn new(token_store: Arc<dyn TokenStore>) -> Self {
        Self {
            token_store,
            handlers: RwLock::new(HashMap::new()),
            connection_locks: Mutex::new(HashMap::new()),
        }
    }

    #[allow(dead_code)]
    pub fn register(
        &self,
        provider_id: impl Into<String>,
        handler: Arc<dyn RefreshHandler>,
    ) -> Result<(), RefreshServiceError> {
        self.handlers
            .write()
            .map_err(|_| RefreshServiceError::Lock)?
            .insert(provider_id.into(), handler);
        Ok(())
    }

    pub async fn refresh_scheduled(
        &self,
        db: &Db,
        connection_id: &str,
    ) -> Result<RefreshOutcome, RefreshServiceError> {
        self.refresh(db, connection_id, false).await
    }

    #[allow(dead_code)]
    pub async fn refresh_on_demand(
        &self,
        db: &Db,
        connection_id: &str,
    ) -> Result<RefreshOutcome, RefreshServiceError> {
        self.refresh(db, connection_id, true).await
    }

    async fn refresh(
        &self,
        db: &Db,
        connection_id: &str,
        force: bool,
    ) -> Result<RefreshOutcome, RefreshServiceError> {
        let _guard = self.lock_connection(connection_id).await?;

        // Re-read under the per-connection lock so a waiting refresh observes
        // rotation or revoke state produced by the previous operation.
        let connection = db
            .get_app_connection(connection_id)
            .map_err(|_| RefreshServiceError::Database)?
            .ok_or(RefreshServiceError::NotFound)?;
        if connection.status == ConnectionStatus::Revoked {
            return Err(RefreshServiceError::Revoked);
        }
        let handler = self
            .handlers
            .read()
            .map_err(|_| RefreshServiceError::Lock)?
            .get(&connection.provider_id)
            .cloned()
            .ok_or(RefreshServiceError::NotRegistered)?;
        if !force && !handler.needs_refresh(&connection, Utc::now()) {
            return Ok(RefreshOutcome::NotNeeded);
        }

        let credential_ref = connection.credential_ref.clone();
        let store = self.token_store.clone();
        let credential = tauri::async_runtime::spawn_blocking(move || store.get(&credential_ref))
            .await
            .map_err(|_| RefreshServiceError::Lock)?;
        let credential = match credential {
            Ok(value) => value,
            Err(error) => {
                let (status, code) = match error {
                    TokenStoreError::Missing | TokenStoreError::Invalid => {
                        (ConnectionStatus::Expired, "credential_missing")
                    }
                    TokenStoreError::Locked | TokenStoreError::Failed => {
                        (ConnectionStatus::Error, "credential_store_locked")
                    }
                };
                db.set_app_connection_refresh_state(&connection.id, status, None, Some(code))
                    .map_err(|_| RefreshServiceError::Database)?;
                return Err(RefreshServiceError::TokenStore(error));
            }
        };

        let refreshed = match handler.refresh(&connection, credential).await {
            Ok(value) => value,
            Err(error) => {
                let status = match error.kind() {
                    RefreshFailureKind::Retryable => ConnectionStatus::Error,
                    RefreshFailureKind::Terminal => ConnectionStatus::Expired,
                };
                db.set_app_connection_refresh_state(
                    &connection.id,
                    status,
                    connection.expires_at.as_deref(),
                    Some(error.code()),
                )
                .map_err(|_| RefreshServiceError::Database)?;
                return Err(RefreshServiceError::Provider(error));
            }
        };

        let expires_at = refreshed.expires_at.clone();
        let credential_ref = connection.credential_ref.clone();
        let store = self.token_store.clone();
        let persisted =
            tauri::async_runtime::spawn_blocking(move || store.put(&credential_ref, &refreshed))
                .await
                .map_err(|_| RefreshServiceError::Lock)?;
        if let Err(error) = persisted {
            let code = match error {
                TokenStoreError::Missing | TokenStoreError::Invalid => "credential_missing",
                TokenStoreError::Locked | TokenStoreError::Failed => "credential_store_locked",
            };
            db.set_app_connection_refresh_state(
                &connection.id,
                ConnectionStatus::Error,
                connection.expires_at.as_deref(),
                Some(code),
            )
            .map_err(|_| RefreshServiceError::Database)?;
            return Err(RefreshServiceError::TokenStore(error));
        }
        db.set_app_connection_refresh_state(
            &connection.id,
            ConnectionStatus::Connected,
            expires_at.as_deref(),
            None,
        )
        .map_err(|_| RefreshServiceError::Database)?;
        Ok(RefreshOutcome::Refreshed)
    }

    pub(crate) async fn lock_connection(
        &self,
        connection_id: &str,
    ) -> Result<OwnedMutexGuard<()>, RefreshServiceError> {
        let lock = {
            let mut locks = self
                .connection_locks
                .lock()
                .map_err(|_| RefreshServiceError::Lock)?;
            locks
                .entry(connection_id.to_owned())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        Ok(lock.lock_owned().await)
    }

    /// Called by the app health loop. Each connection gets a deterministic
    /// provider-aware offset, avoiding a refresh burst after startup.
    pub async fn scheduled_health_check(&self, db: &Db) {
        let Ok(connections) = db.list_app_connections() else {
            return;
        };
        let now = Utc::now();
        for connection in connections {
            if connection.status == ConnectionStatus::Revoked
                || !health_check_is_due(&connection, now)
            {
                continue;
            }
            let _ = self.refresh_scheduled(db, &connection.id).await;
        }
    }
}

fn stable_error_code(value: &str) -> String {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        value.to_owned()
    } else {
        "provider_refresh_failed".into()
    }
}

fn health_check_is_due(connection: &AppConnection, now: DateTime<Utc>) -> bool {
    let Some(last_checked) = connection
        .last_checked_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
    else {
        return true;
    };
    let hash = connection
        .provider_id
        .bytes()
        .chain(connection.id.bytes())
        .fold(0_u64, |value, byte| {
            value.wrapping_mul(31).wrapping_add(byte as u64)
        });
    let jitter = ChronoDuration::seconds((hash % 30) as i64);
    now >= last_checked + ChronoDuration::minutes(5) + jitter
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::{canonical_identity_key, UpsertAppConnection};
    use crate::integrations::token_store::InMemoryTokenStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct FakeHandler {
        needs_refresh: bool,
        failure: Option<ProviderRefreshError>,
        active: Arc<AtomicUsize>,
        maximum_active: Arc<AtomicUsize>,
    }

    impl RefreshHandler for FakeHandler {
        fn needs_refresh(&self, _connection: &AppConnection, _now: DateTime<Utc>) -> bool {
            self.needs_refresh
        }

        fn refresh<'a>(
            &'a self,
            _connection: &'a AppConnection,
            mut credential: CredentialEnvelope,
        ) -> RefreshFuture<'a> {
            Box::pin(async move {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.maximum_active.fetch_max(active, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(15)).await;
                self.active.fetch_sub(1, Ordering::SeqCst);
                if let Some(error) = self.failure.clone() {
                    return Err(error);
                }
                credential.access_token.push_str("-rotated");
                credential.expires_at = Some("2099-01-01T00:00:00Z".into());
                Ok(credential)
            })
        }
    }

    fn connection(db: &Db, credential_ref: &str) -> AppConnection {
        db.upsert_app_connection(UpsertAppConnection {
            provider_id: "slack".into(),
            display_name: Some("Fake".into()),
            external_account_id: Some("account".into()),
            external_tenant_id: None,
            connection_mode: "native_oauth".into(),
            identity_key: canonical_identity_key("slack", "native_oauth", &["account"]),
            scopes: vec![],
            expires_at: None,
            credential_ref: credential_ref.into(),
        })
        .expect("connection")
    }

    fn service(
        store: Arc<InMemoryTokenStore>,
        needs_refresh: bool,
        failure: Option<ProviderRefreshError>,
    ) -> (RefreshService, Arc<AtomicUsize>) {
        let maximum = Arc::new(AtomicUsize::new(0));
        let service = RefreshService::new(store);
        service
            .register(
                "slack",
                Arc::new(FakeHandler {
                    needs_refresh,
                    failure,
                    active: Arc::new(AtomicUsize::new(0)),
                    maximum_active: maximum.clone(),
                }),
            )
            .expect("register");
        (service, maximum)
    }

    #[tokio::test]
    async fn scheduled_and_on_demand_refresh_share_the_same_path() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let item = connection(&db, "credential");
        store
            .put("credential", &CredentialEnvelope::new("token".into()))
            .expect("token");
        let (service, _) = service(store.clone(), true, None);

        assert_eq!(
            service
                .refresh_scheduled(&db, &item.id)
                .await
                .expect("scheduled"),
            RefreshOutcome::Refreshed
        );
        assert_eq!(
            service
                .refresh_on_demand(&db, &item.id)
                .await
                .expect("on demand"),
            RefreshOutcome::Refreshed
        );
        assert_eq!(
            store.get("credential").expect("rotated token").access_token,
            "token-rotated-rotated"
        );
    }

    #[tokio::test]
    async fn rotating_refreshes_are_serialized_per_connection() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let item = connection(&db, "credential");
        store
            .put("credential", &CredentialEnvelope::new("token".into()))
            .expect("token");
        let (service, maximum) = service(store.clone(), true, None);

        let (first, second) = tokio::join!(
            service.refresh_on_demand(&db, &item.id),
            service.refresh_on_demand(&db, &item.id)
        );
        first.expect("first refresh");
        second.expect("second refresh");
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.get("credential").expect("token").access_token,
            "token-rotated-rotated"
        );
    }

    #[tokio::test]
    async fn retryable_and_terminal_failures_own_status_transitions() {
        for (error, expected_status) in [
            (
                ProviderRefreshError::retryable("provider_unavailable"),
                ConnectionStatus::Error,
            ),
            (
                ProviderRefreshError::terminal("grant_revoked"),
                ConnectionStatus::Expired,
            ),
        ] {
            let db = Db::open_in_memory().expect("database");
            let store = Arc::new(InMemoryTokenStore::default());
            let item = connection(&db, "credential");
            store
                .put("credential", &CredentialEnvelope::new("token".into()))
                .expect("token");
            let (service, _) = service(store, true, Some(error.clone()));

            assert!(service.refresh_on_demand(&db, &item.id).await.is_err());
            let updated = db
                .get_app_connection(&item.id)
                .expect("read")
                .expect("connection");
            assert_eq!(updated.status, expected_status);
            assert_eq!(updated.last_error_code.as_deref(), Some(error.code()));
        }
    }

    #[tokio::test]
    async fn refresh_never_resurrects_a_revoked_connection() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let item = connection(&db, "credential");
        store
            .put("credential", &CredentialEnvelope::new("token".into()))
            .expect("token");
        db.mark_app_connection_revoked(&item.id).expect("revoke");
        let (service, _) = service(store, true, None);

        assert!(matches!(
            service.refresh_on_demand(&db, &item.id).await,
            Err(RefreshServiceError::Revoked)
        ));
        assert_eq!(
            db.get_app_connection(&item.id)
                .expect("read")
                .expect("connection")
                .status,
            ConnectionStatus::Revoked
        );
    }

    #[test]
    fn provider_error_codes_are_bounded_and_never_preserve_raw_responses() {
        let error =
            ProviderRefreshError::retryable("Bearer access-secret-fixture from provider response");
        assert_eq!(error.code(), "provider_refresh_failed");
        assert!(!error.to_string().contains("access-secret-fixture"));
    }
}
