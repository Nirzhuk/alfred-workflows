use super::authorization::{
    AuthorizationAttempt, AuthorizationAttemptRegistry, AuthorizationContext,
    AuthorizationRegistryError, AuthorizationStartedDto,
};
use super::credential_store::{
    AgentCredentialEnvelope, AgentCredentialStore, AgentCredentialStoreError,
    OsAgentCredentialStore,
};
use super::models::{
    AgentAccount, AgentAccountCommandError, AgentAccountDto, AgentAccountStatus,
    AgentAuthMethod, AgentProviderRegistrationDto, AuthorizedAgentAccount,
    CredentialCustodyMode,
};
use crate::agents::{AgentHarness, AgentProvider};
use crate::db::Db;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use zeroize::Zeroizing;

/// Upper bound on retained per-account mutexes before idle ones are dropped.
const MAX_TRACKED_ACCOUNT_LOCKS: usize = 64;

/// True when an RFC 3339 expiry is in the past. An unparsable or absent value
/// is treated as "not expired" so a provider quirk cannot lock a user out.
pub fn is_past_expiry(expires_at: Option<&str>) -> bool {
    expires_at
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|expiry| expiry <= chrono::Utc::now())
}

pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentProviderError>> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct AgentProviderRegistration {
    pub provider: AgentProvider,
    pub harness: AgentHarness,
    pub auth_method: AgentAuthMethod,
    pub custody_mode: CredentialCustodyMode,
    pub gate_code: Option<String>,
}

impl AgentProviderRegistration {
    fn dto(&self, handler_available: bool) -> AgentProviderRegistrationDto {
        AgentProviderRegistrationDto {
            provider_id: self.provider.as_str().into(),
            provider_name: self.provider.label().into(),
            harness: self.harness,
            auth_methods: Vec::new(),
            billing_source: "unavailable".into(),
            credential_custody: "unavailable".into(),
            connect_available: handler_available && self.gate_code.is_none(),
            gate_code: self.gate_code.clone(),
        }
    }
}

pub struct ProviderAuthorizationStart {
    pub authorization_url: Option<String>,
    pub user_code: Option<String>,
    pub ttl: Duration,
    pub context: AuthorizationContext,
}

pub struct ProviderAccountGrant {
    pub account: AuthorizedAgentAccount,
    pub credential: AgentCredentialEnvelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Provider plans construct each classified failure kind.
pub enum AgentProviderFailureKind {
    Retryable,
    TerminalRevoked,
    UnsupportedAuthMode,
    PolicyDenied,
}

#[derive(Debug, Clone)]
pub struct AgentProviderError {
    pub code: String,
    pub kind: AgentProviderFailureKind,
}

impl AgentProviderError {
    #[allow(dead_code)] // Provider plans return stable, classified failures.
    pub fn new(code: &str, kind: AgentProviderFailureKind) -> Self {
        Self {
            code: super::models::stable_error_code(code),
            kind,
        }
    }
}

pub trait AgentAccountProvider: Send + Sync {
    fn registration(&self) -> AgentProviderRegistration;
    fn start_authorization(&self) -> Result<ProviderAuthorizationStart, AgentProviderError>;
    fn complete_authorization<'a>(
        &'a self,
        attempt: AuthorizationAttempt,
    ) -> ProviderFuture<'a, ProviderAccountGrant>;
    fn refresh<'a>(
        &'a self,
        account: &'a AgentAccount,
        credential: AgentCredentialEnvelope,
    ) -> ProviderFuture<'a, AgentCredentialEnvelope>;
    fn revoke<'a>(
        &'a self,
        account: &'a AgentAccount,
        credential: AgentCredentialEnvelope,
    ) -> ProviderFuture<'a, ()>;
}

struct ProviderEntry {
    registration: AgentProviderRegistration,
    handler: Option<Arc<dyn AgentAccountProvider>>,
}

pub struct AgentAccountsState {
    credential_store: Arc<dyn AgentCredentialStore>,
    attempts: AuthorizationAttemptRegistry,
    providers: RwLock<HashMap<String, ProviderEntry>>,
    account_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl Default for AgentAccountsState {
    fn default() -> Self {
        Self::new(Arc::new(OsAgentCredentialStore))
    }
}

impl AgentAccountsState {
    /// Shares the configured store with the native resolver. The secret itself
    /// still never leaves the store's own API.
    pub fn credential_store(&self) -> Arc<dyn AgentCredentialStore> {
        self.credential_store.clone()
    }

    pub fn new(credential_store: Arc<dyn AgentCredentialStore>) -> Self {
        let state = Self {
            credential_store,
            attempts: AuthorizationAttemptRegistry::default(),
            providers: RwLock::new(HashMap::new()),
            account_locks: Mutex::new(HashMap::new()),
        };
        state.register_gated_defaults();
        state
    }

    fn register_gated_defaults(&self) {
        let defaults = [
            (AgentProvider::ClaudeCode, AgentAuthMethod::Runtime, CredentialCustodyMode::RuntimeManaged),
            (AgentProvider::Cursor, AgentAuthMethod::Runtime, CredentialCustodyMode::RuntimeManaged),
            (AgentProvider::Codex, AgentAuthMethod::OAuthPkce, CredentialCustodyMode::AlfredManaged),
            (AgentProvider::Opencode, AgentAuthMethod::Runtime, CredentialCustodyMode::RuntimeManaged),
            (AgentProvider::GithubCopilot, AgentAuthMethod::DeviceCode, CredentialCustodyMode::AlfredManaged),
            (AgentProvider::Gemini, AgentAuthMethod::OAuthPkce, CredentialCustodyMode::AlfredManaged),
            (AgentProvider::Grok, AgentAuthMethod::OAuthPkce, CredentialCustodyMode::AlfredManaged),
            (AgentProvider::Pi, AgentAuthMethod::Runtime, CredentialCustodyMode::RuntimeManaged),
            (AgentProvider::Omp, AgentAuthMethod::Runtime, CredentialCustodyMode::RuntimeManaged),
        ];
        let Ok(mut providers) = self.providers.write() else {
            return;
        };
        for (provider, auth_method, custody_mode) in defaults {
            providers.insert(
                provider.as_str().into(),
                ProviderEntry {
                    registration: AgentProviderRegistration {
                        provider,
                        harness: AgentHarness::Alfred,
                        auth_method,
                        custody_mode,
                        gate_code: Some("native_provider_not_available".into()),
                    },
                    handler: None,
                },
            );
        }
    }

    #[allow(dead_code)]
    pub fn register(&self, handler: Arc<dyn AgentAccountProvider>) -> Result<(), AgentAccountCommandError> {
        let registration = handler.registration();
        if registration.harness != AgentHarness::Alfred {
            return Err(command_error(
                "unsupported_auth_mode",
                "Native accounts must use the Alfred harness.",
                false,
            ));
        }
        self.providers
            .write()
            .map_err(|_| state_unavailable())?
            .insert(
                registration.provider.as_str().into(),
                ProviderEntry {
                    registration,
                    handler: Some(handler),
                },
            );
        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<AgentProviderRegistrationDto>, AgentAccountCommandError> {
        let providers = self.providers.read().map_err(|_| state_unavailable())?;
        let mut values = providers
            .values()
            .map(|entry| entry.registration.dto(entry.handler.is_some()))
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.provider_name.cmp(&right.provider_name));
        Ok(values)
    }

    pub fn list_accounts(&self, db: &Db) -> Result<Vec<AgentAccountDto>, AgentAccountCommandError> {
        let accounts = db.list_agent_accounts().map_err(|_| metadata_error())?;
        Ok(accounts
            .into_iter()
            .map(|account| self.project_expiry(db, account))
            .map(AgentAccountDto::from)
            .collect())
    }

    pub fn get_account(&self, db: &Db, id: &str) -> Result<Option<AgentAccountDto>, AgentAccountCommandError> {
        Ok(db
            .get_agent_account(id)
            .map_err(|_| metadata_error())?
            .map(|account| self.project_expiry(db, account))
            .map(AgentAccountDto::from))
    }

    /// A connected account whose credential expiry has passed is reported as
    /// `expired`, and the honest state is persisted so later reads agree.
    fn project_expiry(&self, db: &Db, mut account: AgentAccount) -> AgentAccount {
        if account.status != AgentAccountStatus::Connected
            || !is_past_expiry(account.expires_at.as_deref())
        {
            return account;
        }
        let _ = db.set_agent_account_state(
            &account.id,
            AgentAccountStatus::Expired,
            account.expires_at.as_deref(),
            Some("credential_expired"),
        );
        account.status = AgentAccountStatus::Expired;
        account.last_error_code = Some("credential_expired".into());
        account
    }

    pub fn start_authorization(
        &self,
        provider_id: &str,
        harness: AgentHarness,
    ) -> Result<AuthorizationStartedDto, AgentAccountCommandError> {
        let provider = AgentProvider::from_str(provider_id).ok_or_else(|| {
            command_error("provider_not_found", "The native agent provider is unknown.", false)
        })?;
        let (registration, handler) = self.provider(provider)?;
        if harness != registration.harness {
            return Err(command_error(
                "provider_mismatch",
                "The provider is not registered for this harness.",
                false,
            ));
        }
        if let Some(code) = registration.gate_code.as_deref() {
            return Err(command_error(
                code,
                "Native authorization for this provider is not available in this build.",
                false,
            ));
        }
        let handler = handler.ok_or_else(|| {
            command_error(
                "native_provider_not_available",
                "Native authorization for this provider is not available in this build.",
                false,
            )
        })?;
        let started = handler.start_authorization().map_err(provider_command_error)?;
        let (attempt_id, expires_at) = self
            .attempts
            .insert(
                provider,
                harness,
                registration.auth_method,
                started.ttl,
                started.context,
            )
            .map_err(registry_command_error)?;
        Ok(AuthorizationStartedDto {
            attempt_id,
            provider_id: provider.as_str().into(),
            authorization_url: started.authorization_url,
            user_code: started.user_code,
            expires_at: expires_at.to_rfc3339(),
        })
    }

    pub async fn complete_authorization(
        &self,
        db: &Db,
        attempt_id: &str,
        provider_id: &str,
        harness: AgentHarness,
        completion_state: Option<&str>,
    ) -> Result<AgentAccountDto, AgentAccountCommandError> {
        let provider = AgentProvider::from_str(provider_id).ok_or_else(|| {
            command_error("provider_not_found", "The native agent provider is unknown.", false)
        })?;
        let (registration, handler) = self.provider(provider)?;
        let handler = handler.ok_or_else(|| {
            command_error("native_provider_not_available", "This provider cannot complete authorization.", false)
        })?;
        let attempt = self
            .attempts
            .take(attempt_id, provider, harness, completion_state)
            .map_err(registry_command_error)?;
        let completion = handler.complete_authorization(attempt).await;
        self.attempts.finish(attempt_id);
        let grant = completion.map_err(provider_command_error)?;
        if grant.account.provider != provider
            || grant.account.harness != harness
            || grant.account.auth_method != registration.auth_method
            || grant.account.custody_mode != registration.custody_mode
            || grant.credential.custody_mode != registration.custody_mode
        {
            return Err(command_error(
                "provider_mismatch",
                "The provider returned account data for a different registration.",
                false,
            ));
        }

        let account = db.prepare_agent_account(grant.account).map_err(|_| metadata_error())?;
        let credential_ref = account.credential_ref.clone();
        let store = self.credential_store.clone();
        let expires_at = grant.credential.expires_at.clone();
        let persisted = tauri::async_runtime::spawn_blocking(move || {
            store.put(&credential_ref, &grant.credential)
        })
        .await
        .map_err(|_| state_unavailable())?;
        if let Err(error) = persisted {
            let code = credential_error_code(error);
            let _ = db.set_agent_account_state(
                &account.id,
                AgentAccountStatus::Error,
                account.expires_at.as_deref(),
                Some(code),
            );
            return Err(credential_command_error(error));
        }
        db.set_agent_account_state(
            &account.id,
            AgentAccountStatus::Connected,
            expires_at.as_deref(),
            None,
        )
        .map_err(|_| metadata_error())?;
        db.get_agent_account(&account.id)
            .map_err(|_| metadata_error())?
            .map(AgentAccountDto::from)
            .ok_or_else(metadata_error)
    }

    pub fn cancel_authorization(&self, attempt_id: &str) -> Result<(), AgentAccountCommandError> {
        self.attempts.cancel(attempt_id).map_err(registry_command_error)
    }

    pub async fn refresh_account(
        &self,
        db: &Db,
        account_id: &str,
    ) -> Result<AgentAccountDto, AgentAccountCommandError> {
        let _guard = self.lock_account(account_id).await?;
        let account = db
            .get_agent_account(account_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(account_not_found)?;
        if matches!(account.status, AgentAccountStatus::Revoked | AgentAccountStatus::DisconnectPending) {
            return Err(command_error(
                "account_not_refreshable",
                "This account must finish disconnecting or reconnect.",
                false,
            ));
        }
        let (_, handler) = self.provider(account.provider)?;
        let Some(handler) = handler else {
            db.set_agent_account_state(
                &account.id,
                AgentAccountStatus::Error,
                account.expires_at.as_deref(),
                Some("unsupported_auth_mode"),
            )
            .map_err(|_| metadata_error())?;
            return Err(command_error(
                "unsupported_auth_mode",
                "This build cannot refresh the account's authorization method.",
                false,
            ));
        };
        let credential = self.read_credential(&account).await?;
        let refreshed = match handler.refresh(&account, credential).await {
            Ok(credential) => credential,
            Err(error) => {
                self.record_provider_failure(db, &account, &error)?;
                return Err(provider_command_error(error));
            }
        };

        let expires_at = refreshed.expires_at.clone();
        let credential_ref = account.credential_ref.clone();
        let store = self.credential_store.clone();
        let persisted = tauri::async_runtime::spawn_blocking(move || store.put(&credential_ref, &refreshed))
            .await
            .map_err(|_| state_unavailable())?;
        if let Err(error) = persisted {
            db.set_agent_account_state(
                &account.id,
                AgentAccountStatus::Error,
                account.expires_at.as_deref(),
                Some(credential_error_code(error)),
            )
            .map_err(|_| metadata_error())?;
            return Err(credential_command_error(error));
        }
        // This promotion happens only after the rotated credential is durable.
        db.set_agent_account_state(
            &account.id,
            AgentAccountStatus::Connected,
            expires_at.as_deref(),
            None,
        )
        .map_err(|_| metadata_error())?;
        self.get_account(db, &account.id)?.ok_or_else(account_not_found)
    }

    /// Dedicated intake for Alfred-managed API keys. Runtime registration is
    /// intentionally not involved: the native runtimes remain gated until
    /// their independent release evidence passes.
    pub async fn connect_api_key_account(
        &self,
        db: &Db,
        provider: AgentProvider,
        account_id: Option<&str>,
        api_key: Zeroizing<String>,
    ) -> Result<AgentAccountDto, AgentAccountCommandError> {
        validate_native_api_key(provider, api_key.as_str())?;
        let fingerprint = api_key_fingerprint(provider, api_key.as_str());
        let metadata = AuthorizedAgentAccount {
            provider,
            harness: AgentHarness::Alfred,
            display_name: Some("API key".into()),
            external_account_id: format!("api-key-sha256:{fingerprint}"),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            scopes: Vec::new(),
            expires_at: None,
        };
        let requested = match account_id {
            Some(id) => db.get_agent_account(id).map_err(|_| metadata_error())?,
            None => db
                .get_agent_account_by_identity(
                    provider,
                    AgentHarness::Alfred,
                    &metadata.identity_key(),
                )
                .map_err(|_| metadata_error())?,
        };
        let (account, is_new) = if let Some(account) = requested {
            if account.provider != provider
                || account.harness != AgentHarness::Alfred
                || account.auth_method != AgentAuthMethod::ApiKey
                || account.custody_mode != CredentialCustodyMode::AlfredManaged
            {
                return Err(command_error(
                    "provider_mismatch",
                    "That account cannot be reconnected with this provider credential.",
                    false,
                ));
            }
            (account, false)
        } else if account_id.is_some() {
            return Err(account_not_found());
        } else {
            (
                db.prepare_agent_account(copy_api_key_metadata(&metadata))
                    .map_err(|_| metadata_error())?,
                true,
            )
        };

        let _guard = self.lock_account(&account.id).await?;
        // `prepare_agent_account` may reuse the same pending identity while a
        // concurrent connect is waiting for this lock. Re-read after locking
        // so the waiter backs up a credential finalized by the first writer
        // instead of treating that durable account as disposable new state.
        let account = db
            .get_agent_account(&account.id)
            .map_err(|_| metadata_error())?
            .ok_or_else(account_not_found)?;
        let is_new = is_new && account.status != AgentAccountStatus::Connected;
        let previous = if is_new {
            None
        } else {
            let store = self.credential_store.clone();
            let credential_ref = account.credential_ref.clone();
            match tauri::async_runtime::spawn_blocking(move || store.get(&credential_ref))
                .await
                .map_err(|_| state_unavailable())?
            {
                Ok(credential) => Some(credential),
                Err(AgentCredentialStoreError::Missing | AgentCredentialStoreError::Invalid) => {
                    None
                }
                Err(error) => return Err(credential_command_error(error)),
            }
        };

        let credential_ref = account.credential_ref.clone();
        let store = self.credential_store.clone();
        let credential = AgentCredentialEnvelope::alfred_managed(api_key.as_str().to_owned());
        let persisted = tauri::async_runtime::spawn_blocking(move || {
            store.put(&credential_ref, &credential)
        })
        .await
        .map_err(|_| state_unavailable())?;
        if let Err(error) = persisted {
            let original_error = credential_command_error(error);
            if let Err(rollback_error) = rollback_api_key_credential(
                self.credential_store.clone(),
                account.credential_ref.clone(),
                previous,
            )
            .await
            {
                let _ = db.set_agent_account_state(
                    &account.id,
                    AgentAccountStatus::Error,
                    None,
                    Some("credential_rollback_failed"),
                );
                return Err(rollback_error);
            }
            if is_new {
                db.delete_agent_account_metadata(&account.id)
                    .map_err(|_| metadata_error())?;
                self.release_account_lock(&account.id);
            }
            return Err(original_error);
        }

        if db.finalize_api_key_agent_account(&account.id, &metadata).is_err() {
            if let Err(rollback_error) = rollback_api_key_credential(
                self.credential_store.clone(),
                account.credential_ref.clone(),
                previous,
            )
            .await
            {
                let _ = db.set_agent_account_state(
                    &account.id,
                    AgentAccountStatus::Error,
                    None,
                    Some("credential_rollback_failed"),
                );
                return Err(rollback_error);
            }
            if is_new {
                db.delete_agent_account_metadata(&account.id)
                    .map_err(|_| metadata_error())?;
                self.release_account_lock(&account.id);
            }
            return Err(metadata_error());
        }

        self.get_account(db, &account.id)?.ok_or_else(account_not_found)
    }

    pub async fn disconnect_account(
        &self,
        db: &Db,
        account_id: &str,
        metadata_only: bool,
    ) -> Result<(), AgentAccountCommandError> {
        let _guard = self.lock_account(account_id).await?;
        let account = db
            .get_agent_account(account_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(account_not_found)?;
        if metadata_only {
            // Best-effort credential removal first: the credential reference
            // lives only on this row, so deleting metadata first would strand
            // the secret in the OS store with no way to find it again.
            let credential_ref = account.credential_ref.clone();
            let store = self.credential_store.clone();
            let deletion = tauri::async_runtime::spawn_blocking(move || store.delete(&credential_ref))
                .await
                .map_err(|_| state_unavailable())?;
            match deletion {
                Ok(()) | Err(AgentCredentialStoreError::Missing) => {}
                Err(error) => {
                    // Surface the store failure rather than orphaning a secret.
                    let _ = db.set_agent_account_state(
                        account_id,
                        AgentAccountStatus::DisconnectPending,
                        account.expires_at.as_deref(),
                        Some(credential_error_code(error)),
                    );
                    return Err(credential_command_error(error));
                }
            }
            let result = db
                .delete_agent_account_metadata(account_id)
                .map_err(|_| metadata_error());
            if result.is_ok() {
                self.release_account_lock(account_id);
            }
            return result;
        }

        let credential = match self.read_credential(&account).await {
            Ok(credential) => Some(credential),
            Err(error) if error.code == "credential_missing" => None,
            Err(error) => {
                let _ = db.set_agent_account_state(
                    account_id,
                    AgentAccountStatus::DisconnectPending,
                    account.expires_at.as_deref(),
                    Some(&error.code),
                );
                return Err(error);
            }
        };
        if let Some(credential) = credential {
            if account.auth_method == AgentAuthMethod::ApiKey {
                // Provider consoles own API-key revocation. Alfred truthfully
                // performs local deletion only and never claims remote revoke.
                drop(credential);
            } else {
                let (_, handler) = self.provider(account.provider)?;
                let Some(handler) = handler else {
                    db.set_agent_account_state(
                        account_id,
                        AgentAccountStatus::DisconnectPending,
                        account.expires_at.as_deref(),
                        Some("unsupported_auth_mode"),
                    )
                    .map_err(|_| metadata_error())?;
                    return Err(command_error(
                        "unsupported_auth_mode",
                        "This build cannot revoke the provider credential. Retry with provider support or remove local metadata after revoking it yourself.",
                        true,
                    ));
                };
                if let Err(error) = handler.revoke(&account, credential).await {
                    if error.kind != AgentProviderFailureKind::TerminalRevoked {
                        db.set_agent_account_state(
                            account_id,
                            AgentAccountStatus::DisconnectPending,
                            account.expires_at.as_deref(),
                            Some(&error.code),
                        )
                        .map_err(|_| metadata_error())?;
                        return Err(provider_command_error(error));
                    }
                }
            }
        }

        db.set_agent_account_state(
            account_id,
            AgentAccountStatus::Revoked,
            account.expires_at.as_deref(),
            None,
        )
        .map_err(|_| metadata_error())?;
        let credential_ref = account.credential_ref.clone();
        let store = self.credential_store.clone();
        let deletion = tauri::async_runtime::spawn_blocking(move || store.delete(&credential_ref))
            .await
            .map_err(|_| state_unavailable())?;
        if let Err(error) = deletion {
            if error != AgentCredentialStoreError::Missing {
                db.set_agent_account_state(
                    account_id,
                    AgentAccountStatus::DisconnectPending,
                    account.expires_at.as_deref(),
                    Some(credential_error_code(error)),
                )
                .map_err(|_| metadata_error())?;
                return Err(credential_command_error(error));
            }
        }
        let result = db.delete_agent_account_metadata(account_id).map_err(|_| {
            command_error(
                "metadata_delete_failed",
                "The credential was removed, but local account metadata remains revoked. Retry disconnecting.",
                true,
            )
        });
        if result.is_ok() {
            self.release_account_lock(account_id);
        }
        result
    }

    /// Drops the per-account mutex once the account no longer exists so the
    /// map cannot grow for the life of the process.
    fn release_account_lock(&self, account_id: &str) {
        if let Ok(mut locks) = self.account_locks.lock() {
            locks.remove(account_id);
            if locks.len() > MAX_TRACKED_ACCOUNT_LOCKS {
                // A live guard holds its own Arc clone, so anything with a
                // single reference is idle and safe to drop.
                locks.retain(|_, lock| Arc::strong_count(lock) > 1);
            }
        }
    }

    fn provider(
        &self,
        provider: AgentProvider,
    ) -> Result<(AgentProviderRegistration, Option<Arc<dyn AgentAccountProvider>>), AgentAccountCommandError> {
        let providers = self.providers.read().map_err(|_| state_unavailable())?;
        let entry = providers.get(provider.as_str()).ok_or_else(|| {
            command_error("provider_not_found", "The native agent provider is unknown.", false)
        })?;
        Ok((entry.registration.clone(), entry.handler.clone()))
    }

    async fn lock_account(&self, account_id: &str) -> Result<OwnedMutexGuard<()>, AgentAccountCommandError> {
        let lock = {
            let mut locks = self.account_locks.lock().map_err(|_| state_unavailable())?;
            if let Some(lock) = locks.get(account_id) {
                lock.clone()
            } else {
                if locks.len() >= MAX_TRACKED_ACCOUNT_LOCKS {
                    // The map owns one strong reference. Owned guards and
                    // waiters own additional references, so pruning only
                    // single-reference entries cannot evict an in-use lock.
                    locks.retain(|_, lock| Arc::strong_count(lock) > 1);
                }
                if locks.len() >= MAX_TRACKED_ACCOUNT_LOCKS {
                    return Err(command_error(
                        "account_operation_busy",
                        "Too many native-agent account operations are active. Retry shortly.",
                        true,
                    ));
                }
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(account_id.into(), lock.clone());
                lock
            }
        };
        Ok(lock.lock_owned().await)
    }

    async fn read_credential(
        &self,
        account: &AgentAccount,
    ) -> Result<AgentCredentialEnvelope, AgentAccountCommandError> {
        let store = self.credential_store.clone();
        let credential_ref = account.credential_ref.clone();
        let result = tauri::async_runtime::spawn_blocking(move || store.get(&credential_ref))
            .await
            .map_err(|_| state_unavailable())?;
        result.map_err(credential_command_error)
    }

    fn record_provider_failure(
        &self,
        db: &Db,
        account: &AgentAccount,
        error: &AgentProviderError,
    ) -> Result<(), AgentAccountCommandError> {
        let status = match error.kind {
            AgentProviderFailureKind::Retryable | AgentProviderFailureKind::PolicyDenied => {
                AgentAccountStatus::Error
            }
            AgentProviderFailureKind::TerminalRevoked => AgentAccountStatus::Revoked,
            AgentProviderFailureKind::UnsupportedAuthMode => AgentAccountStatus::Error,
        };
        db.set_agent_account_state(
            &account.id,
            status,
            account.expires_at.as_deref(),
            Some(&error.code),
        )
        .map_err(|_| metadata_error())
    }
}

fn registry_command_error(error: AuthorizationRegistryError) -> AgentAccountCommandError {
    match error {
        AuthorizationRegistryError::NotFound => command_error("authorization_not_found", "The authorization attempt no longer exists. Start again.", false),
        AuthorizationRegistryError::Expired => command_error("authorization_expired", "The authorization attempt expired. Start again.", false),
        AuthorizationRegistryError::Cancelled => command_error("authorization_cancelled", "The authorization attempt was cancelled.", false),
        AuthorizationRegistryError::StateMismatch => command_error("authorization_state_mismatch", "The authorization response could not be verified.", false),
        AuthorizationRegistryError::StateRequired => command_error("authorization_state_required", "OAuth authorization could not start without a secure state value.", false),
        AuthorizationRegistryError::ProviderMismatch => command_error("provider_mismatch", "The authorization response belongs to a different provider.", false),
        AuthorizationRegistryError::Busy => command_error("authorization_busy", "Too many native authorization attempts are active.", true),
        AuthorizationRegistryError::Lock => state_unavailable(),
    }
}

fn provider_command_error(error: AgentProviderError) -> AgentAccountCommandError {
    let (message, recoverable) = match error.kind {
        AgentProviderFailureKind::Retryable => ("The provider is temporarily unavailable. Try again.", true),
        AgentProviderFailureKind::TerminalRevoked => ("The provider revoked this account. Reconnect it.", false),
        AgentProviderFailureKind::UnsupportedAuthMode => ("This build does not support the account's authorization method.", false),
        AgentProviderFailureKind::PolicyDenied => ("The provider policy denied this operation.", false),
    };
    command_error(&error.code, message, recoverable)
}

fn credential_error_code(error: AgentCredentialStoreError) -> &'static str {
    match error {
        AgentCredentialStoreError::Missing => "credential_missing",
        AgentCredentialStoreError::Locked => "credential_store_locked",
        AgentCredentialStoreError::Invalid => "credential_invalid",
        AgentCredentialStoreError::Failed => "credential_store_failed",
    }
}

fn credential_command_error(error: AgentCredentialStoreError) -> AgentAccountCommandError {
    let code = credential_error_code(error);
    let message = match error {
        AgentCredentialStoreError::Missing => "The native-agent credential is missing. Reconnect the account.",
        AgentCredentialStoreError::Locked => "Unlock the system credential store and try again.",
        AgentCredentialStoreError::Invalid => "The saved native-agent credential is invalid. Reconnect the account.",
        AgentCredentialStoreError::Failed => "The native-agent credential could not be saved or removed.",
    };
    command_error(code, message, true)
}

fn command_error(code: &str, message: &str, recoverable: bool) -> AgentAccountCommandError {
    AgentAccountCommandError::new(code, message, recoverable)
}

fn metadata_error() -> AgentAccountCommandError {
    command_error("account_store_failed", "Native-agent account metadata could not be read or updated.", true)
}

fn state_unavailable() -> AgentAccountCommandError {
    command_error("agent_account_state_unavailable", "Native-agent account state is temporarily unavailable.", true)
}

fn account_not_found() -> AgentAccountCommandError {
    command_error("account_not_found", "The native-agent account no longer exists.", false)
}

async fn rollback_api_key_credential(
    store: Arc<dyn AgentCredentialStore>,
    credential_ref: String,
    previous: Option<AgentCredentialEnvelope>,
) -> Result<(), AgentAccountCommandError> {
    let rollback = tauri::async_runtime::spawn_blocking(move || match previous {
        Some(previous) => store.put(&credential_ref, &previous),
        None => match store.delete(&credential_ref) {
            Ok(()) | Err(AgentCredentialStoreError::Missing) => Ok(()),
            Err(error) => Err(error),
        },
    })
    .await
    .map_err(|_| {
        command_error(
            "credential_rollback_failed",
            "The previous native-agent credential could not be restored.",
            true,
        )
    })?;
    rollback.map_err(|_| {
        command_error(
            "credential_rollback_failed",
            "The previous native-agent credential could not be restored.",
            true,
        )
    })
}

fn copy_api_key_metadata(input: &AuthorizedAgentAccount) -> AuthorizedAgentAccount {
    AuthorizedAgentAccount {
        provider: input.provider,
        harness: input.harness,
        display_name: input.display_name.clone(),
        external_account_id: input.external_account_id.clone(),
        external_workspace_id: None,
        auth_method: input.auth_method,
        custody_mode: input.custody_mode,
        scopes: Vec::new(),
        expires_at: None,
    }
}

fn api_key_fingerprint(provider: AgentProvider, api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(api_key.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn validate_native_api_key(
    provider: AgentProvider,
    api_key: &str,
) -> Result<(), AgentAccountCommandError> {
    let bounded = api_key.len() <= 512 && api_key.trim() == api_key;
    let valid = match provider {
        AgentProvider::ClaudeCode => {
            bounded
                && api_key.starts_with("sk-ant-")
                && api_key.len() > "sk-ant-".len()
                && api_key.bytes().all(|byte| byte.is_ascii_graphic())
        }
        AgentProvider::Gemini => {
            bounded
                && api_key.len() >= 20
                && api_key.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                })
        }
        AgentProvider::Grok => {
            bounded
                && api_key.len() >= 20
                && api_key.starts_with("xai-")
                && !api_key.chars().any(char::is_whitespace)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(command_error(
            "api_key_invalid",
            "Use a valid API key for this native provider.",
            false,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::credential_store::InMemoryAgentCredentialStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OAUTH_STATE: &str = "state-fixture";

    struct FakeProvider {
        active: Arc<AtomicUsize>,
        maximum: Arc<AtomicUsize>,
        refresh_failure: Mutex<Option<AgentProviderError>>,
        revoke_failure: Mutex<Option<AgentProviderError>>,
        expected_state: Mutex<Option<String>>,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                active: Arc::new(AtomicUsize::new(0)),
                maximum: Arc::new(AtomicUsize::new(0)),
                refresh_failure: Mutex::new(None),
                revoke_failure: Mutex::new(None),
                expected_state: Mutex::new(Some(OAUTH_STATE.into())),
            }
        }
    }

    impl AgentAccountProvider for FakeProvider {
        fn registration(&self) -> AgentProviderRegistration {
            AgentProviderRegistration {
                provider: AgentProvider::Codex,
                harness: AgentHarness::Alfred,
                auth_method: AgentAuthMethod::OAuthPkce,
                custody_mode: CredentialCustodyMode::AlfredManaged,
                gate_code: None,
            }
        }

        fn start_authorization(&self) -> Result<ProviderAuthorizationStart, AgentProviderError> {
            Ok(ProviderAuthorizationStart {
                authorization_url: Some("https://provider.invalid/authorize".into()),
                user_code: None,
                ttl: Duration::from_secs(60),
                context: AuthorizationContext {
                    expected_state: self.expected_state.lock().expect("state lock").clone(),
                    pkce_verifier: None,
                    nonce: None,
                    provider_fields: Default::default(),
                },
            })
        }

        fn complete_authorization<'a>(&'a self, _attempt: AuthorizationAttempt) -> ProviderFuture<'a, ProviderAccountGrant> {
            Box::pin(async move {
                let mut credential =
                    AgentCredentialEnvelope::alfred_managed("first-secret".into());
                credential.expires_at = Some("2099-01-01T00:00:00Z".into());
                Ok(ProviderAccountGrant {
                    account: AuthorizedAgentAccount {
                        provider: AgentProvider::Codex,
                        harness: AgentHarness::Alfred,
                        display_name: Some("Codex User".into()),
                        external_account_id: "user-1".into(),
                        external_workspace_id: Some("workspace-1".into()),
                        auth_method: AgentAuthMethod::OAuthPkce,
                        custody_mode: CredentialCustodyMode::AlfredManaged,
                        scopes: vec!["models:read".into()],
                        expires_at: Some("2099-01-01T00:00:00Z".into()),
                    },
                    credential,
                })
            })
        }

        fn refresh<'a>(&'a self, _account: &'a AgentAccount, mut credential: AgentCredentialEnvelope) -> ProviderFuture<'a, AgentCredentialEnvelope> {
            Box::pin(async move {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.maximum.fetch_max(active, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(15)).await;
                self.active.fetch_sub(1, Ordering::SeqCst);
                if let Some(error) = self.refresh_failure.lock().expect("failure lock").clone() {
                    return Err(error);
                }
                credential.access_token = Some("rotated-secret".into());
                credential.expires_at = Some("2100-01-01T00:00:00Z".into());
                Ok(credential)
            })
        }

        fn revoke<'a>(&'a self, _account: &'a AgentAccount, _credential: AgentCredentialEnvelope) -> ProviderFuture<'a, ()> {
            Box::pin(async move {
                if let Some(error) = self.revoke_failure.lock().expect("failure lock").clone() {
                    Err(error)
                } else {
                    Ok(())
                }
            })
        }
    }

    async fn connected_fixture() -> (Db, Arc<InMemoryAgentCredentialStore>, AgentAccountsState, Arc<FakeProvider>, AgentAccountDto) {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let state = AgentAccountsState::new(store.clone());
        let provider = Arc::new(FakeProvider::new());
        state.register(provider.clone()).expect("register");
        let started = state.start_authorization("codex", AgentHarness::Alfred).expect("start");
        let account = state
            .complete_authorization(
                &db,
                &started.attempt_id,
                "codex",
                AgentHarness::Alfred,
                Some(OAUTH_STATE),
            )
            .await
            .expect("complete");
        (db, store, state, provider, account)
    }

    fn api_key(provider: AgentProvider, suffix: &str) -> String {
        match provider {
            AgentProvider::ClaudeCode => format!("sk-ant-api03-{suffix}-fixture-value"),
            AgentProvider::Gemini => format!("AIza{suffix}FixtureValue1234567890"),
            AgentProvider::Grok => format!("xai-{suffix}-fixture-value-1234567890"),
            _ => panic!("unsupported API-key fixture provider"),
        }
    }

    fn fail_api_key_metadata_updates(db: &Db) {
        db.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER fail_api_key_metadata
                 BEFORE UPDATE OF status ON agent_accounts
                 WHEN NEW.auth_method = 'api_key' AND NEW.status = 'connected'
                 BEGIN SELECT RAISE(FAIL, 'metadata write failed'); END;",
            )?;
            Ok(())
        })
        .expect("install metadata failure");
    }

    #[tokio::test]
    async fn api_key_intake_connects_only_approved_providers_and_serializes_redacted_metadata() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let state = AgentAccountsState::new(store.clone());

        for provider in [
            AgentProvider::ClaudeCode,
            AgentProvider::Gemini,
            AgentProvider::Grok,
        ] {
            let secret = api_key(provider, "success-secret");
            let account = state
                .connect_api_key_account(
                    &db,
                    provider,
                    None,
                    Zeroizing::new(secret.clone()),
                )
                .await
                .expect("connect API key");
            assert_eq!(account.auth_method, AgentAuthMethod::ApiKey);
            assert_eq!(account.custody_mode, CredentialCustodyMode::AlfredManaged);
            assert_eq!(account.status, AgentAccountStatus::Connected);
            assert!(account.external_account_id.is_none());
            assert!(account.external_workspace_id.is_none());

            let serialized = serde_json::to_string(&account).expect("serialize DTO");
            let backend = db
                .get_agent_account(&account.id)
                .expect("read account")
                .expect("saved account");
            assert!(!serialized.contains(&secret));
            assert!(!format!("{backend:?}").contains(&secret));
            assert_eq!(
                store
                    .get(&backend.credential_ref)
                    .expect("stored key")
                    .access_token
                    .as_deref(),
                Some(secret.as_str())
            );
        }
        assert_eq!(db.list_agent_accounts().expect("list").len(), 3);

        for provider in [
            AgentProvider::ClaudeCode,
            AgentProvider::Gemini,
            AgentProvider::Grok,
        ] {
            let invalid_secret = " invalid-secret ".to_string();
            let error = state
                .connect_api_key_account(
                    &db,
                    provider,
                    None,
                    Zeroizing::new(invalid_secret.clone()),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, "api_key_invalid");
            assert!(!format!("{error:?}").contains(&invalid_secret));
        }

        let invalid = state
            .connect_api_key_account(
                &db,
                AgentProvider::Codex,
                None,
                Zeroizing::new("sk-not-approved-secret".into()),
            )
            .await
            .unwrap_err();
        assert_eq!(invalid.code, "api_key_invalid");
        assert!(!format!("{invalid:?}").contains("sk-not-approved-secret"));
    }

    #[tokio::test]
    async fn api_key_reconnect_overwrites_in_place_and_refuses_provider_mismatch() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let state = AgentAccountsState::new(store.clone());
        let first_key = api_key(AgentProvider::Grok, "first-secret");
        let first = state
            .connect_api_key_account(
                &db,
                AgentProvider::Grok,
                None,
                Zeroizing::new(first_key.clone()),
            )
            .await
            .expect("first connect");
        let first_backend = db.get_agent_account(&first.id).unwrap().unwrap();

        let second_key = api_key(AgentProvider::Grok, "second-secret");
        let reconnected = state
            .connect_api_key_account(
                &db,
                AgentProvider::Grok,
                Some(&first.id),
                Zeroizing::new(second_key.clone()),
            )
            .await
            .expect("reconnect");
        assert_eq!(reconnected.id, first.id);
        assert_eq!(db.list_agent_accounts().expect("list").len(), 1);
        assert_eq!(
            store
                .get(&first_backend.credential_ref)
                .expect("overwritten key")
                .access_token
                .as_deref(),
            Some(second_key.as_str())
        );

        let mismatch_key = api_key(AgentProvider::Gemini, "mismatch-secret");
        let error = state
            .connect_api_key_account(
                &db,
                AgentProvider::Gemini,
                Some(&first.id),
                Zeroizing::new(mismatch_key.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "provider_mismatch");
        assert!(!format!("{error:?}").contains(&mismatch_key));
        assert_eq!(
            store
                .get(&first_backend.credential_ref)
                .expect("unchanged key")
                .access_token
                .as_deref(),
            Some(second_key.as_str())
        );

        state
            .disconnect_account(&db, &first.id, false)
            .await
            .expect("local API-key disconnect");
        assert!(db.get_agent_account(&first.id).unwrap().is_none());
        assert_eq!(
            store.get(&first_backend.credential_ref).unwrap_err(),
            AgentCredentialStoreError::Missing
        );
    }

    #[tokio::test]
    async fn api_key_store_and_metadata_failures_roll_back_without_orphaning_secrets() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let state = AgentAccountsState::new(store.clone());
        store.fail_next_put(AgentCredentialStoreError::Locked);
        let rejected_key = api_key(AgentProvider::ClaudeCode, "store-failure-secret");
        let error = state
            .connect_api_key_account(
                &db,
                AgentProvider::ClaudeCode,
                None,
                Zeroizing::new(rejected_key.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "credential_store_locked");
        assert!(db.list_agent_accounts().expect("list").is_empty());
        assert_eq!(store.entry_count(), 0);
        assert!(!format!("{error:?}").contains(&rejected_key));

        store.fail_next_put_after_write(AgentCredentialStoreError::Failed);
        let ambiguous_key = api_key(AgentProvider::ClaudeCode, "ambiguous-failure-secret");
        let error = state
            .connect_api_key_account(
                &db,
                AgentProvider::ClaudeCode,
                None,
                Zeroizing::new(ambiguous_key.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "credential_store_failed");
        assert!(db.list_agent_accounts().expect("list").is_empty());
        assert_eq!(store.entry_count(), 0);
        assert!(!format!("{error:?}").contains(&ambiguous_key));

        fail_api_key_metadata_updates(&db);
        let metadata_key = api_key(AgentProvider::Gemini, "metadata-failure-secret");
        let error = state
            .connect_api_key_account(
                &db,
                AgentProvider::Gemini,
                None,
                Zeroizing::new(metadata_key.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "account_store_failed");
        assert!(db.list_agent_accounts().expect("list").is_empty());
        assert_eq!(store.entry_count(), 0);
        assert!(!format!("{error:?}").contains(&metadata_key));

        store.fail_next_delete(AgentCredentialStoreError::Failed);
        let rollback_key = api_key(AgentProvider::Grok, "rollback-failure-secret");
        let error = state
            .connect_api_key_account(
                &db,
                AgentProvider::Grok,
                None,
                Zeroizing::new(rollback_key.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "credential_rollback_failed");
        let retained = db.list_agent_accounts().expect("list");
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].status, AgentAccountStatus::Error);
        assert_eq!(
            retained[0].last_error_code.as_deref(),
            Some("credential_rollback_failed")
        );
        assert_eq!(store.entry_count(), 1);
        assert!(!format!("{error:?}").contains(&rollback_key));
    }

    #[tokio::test]
    async fn reconnect_metadata_failure_restores_the_previous_key_and_metadata() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let state = AgentAccountsState::new(store.clone());
        let first_key = api_key(AgentProvider::Grok, "rollback-first-secret");
        let first = state
            .connect_api_key_account(
                &db,
                AgentProvider::Grok,
                None,
                Zeroizing::new(first_key.clone()),
            )
            .await
            .expect("first connect");
        let before = db.get_agent_account(&first.id).unwrap().unwrap();
        fail_api_key_metadata_updates(&db);

        let replacement = api_key(AgentProvider::Grok, "rollback-second-secret");
        let error = state
            .connect_api_key_account(
                &db,
                AgentProvider::Grok,
                Some(&first.id),
                Zeroizing::new(replacement.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "account_store_failed");
        assert_eq!(db.get_agent_account(&first.id).unwrap().unwrap(), before);
        assert_eq!(
            store
                .get(&before.credential_ref)
                .expect("restored key")
                .access_token
                .as_deref(),
            Some(first_key.as_str())
        );
        assert!(!format!("{error:?}").contains(&replacement));
    }

    #[test]
    fn default_provider_diagnostics_are_explicit_and_gated() {
        let state = AgentAccountsState::new(Arc::new(InMemoryAgentCredentialStore::default()));
        let providers = state.list_providers().expect("providers");
        assert_eq!(providers.len(), 9);
        assert!(providers.iter().all(|provider| {
            provider.harness == AgentHarness::Alfred
                && !provider.connect_available
                && provider.gate_code.as_deref() == Some("native_provider_not_available")
        }));
        assert!(providers.iter().all(|provider| {
            provider.auth_methods.is_empty()
                && provider.billing_source == "unavailable"
                && provider.credential_custody == "unavailable"
        }));
    }

    #[tokio::test]
    async fn credential_failure_never_promotes_metadata_to_connected() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        store.fail_next_put(AgentCredentialStoreError::Locked);
        let state = AgentAccountsState::new(store);
        state.register(Arc::new(FakeProvider::new())).expect("register");
        let started = state.start_authorization("codex", AgentHarness::Alfred).expect("start");
        let error = state
            .complete_authorization(
                &db,
                &started.attempt_id,
                "codex",
                AgentHarness::Alfred,
                Some(OAUTH_STATE),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "credential_store_locked");
        let saved = db.list_agent_accounts().expect("list").pop().expect("saved metadata");
        assert_eq!(saved.status, AgentAccountStatus::Error);
        assert_eq!(saved.last_error_code.as_deref(), Some("credential_store_locked"));
    }

    #[tokio::test]
    async fn concurrent_refresh_is_serialized_and_rotation_precedes_connected_state() {
        let (db, store, state, provider, account) = connected_fixture().await;
        let (first, second) = tokio::join!(
            state.refresh_account(&db, &account.id),
            state.refresh_account(&db, &account.id)
        );
        first.expect("first refresh");
        second.expect("second refresh");
        assert_eq!(provider.maximum.load(Ordering::SeqCst), 1);
        let backend = db.get_agent_account(&account.id).expect("read").expect("account");
        assert_eq!(backend.status, AgentAccountStatus::Connected);
        assert_eq!(
            store.get(&backend.credential_ref).expect("credential").access_token.as_deref(),
            Some("rotated-secret")
        );
    }

    #[tokio::test]
    async fn refresh_store_failure_keeps_rotated_account_out_of_connected_state() {
        let (db, store, state, _provider, account) = connected_fixture().await;
        let before = db.get_agent_account(&account.id).unwrap().unwrap();
        store.fail_next_put(AgentCredentialStoreError::Locked);

        let error = state.refresh_account(&db, &account.id).await.unwrap_err();
        assert_eq!(error.code, "credential_store_locked");
        let after = db.get_agent_account(&account.id).unwrap().unwrap();
        assert_eq!(after.status, AgentAccountStatus::Error);
        assert_eq!(after.expires_at, before.expires_at);
        assert_eq!(after.last_error_code.as_deref(), Some("credential_store_locked"));
    }

    #[tokio::test]
    async fn terminal_and_retryable_refresh_failures_are_classified() {
        let (db, _store, state, provider, account) = connected_fixture().await;
        *provider.refresh_failure.lock().expect("failure") = Some(AgentProviderError::new(
            "invalid_grant",
            AgentProviderFailureKind::TerminalRevoked,
        ));
        state.refresh_account(&db, &account.id).await.unwrap_err();
        assert_eq!(
            db.get_agent_account(&account.id).unwrap().unwrap().status,
            AgentAccountStatus::Revoked
        );

        db.set_agent_account_state(&account.id, AgentAccountStatus::Connected, None, None).unwrap();
        *provider.refresh_failure.lock().expect("failure") = Some(AgentProviderError::new(
            "provider_unavailable",
            AgentProviderFailureKind::Retryable,
        ));
        state.refresh_account(&db, &account.id).await.unwrap_err();
        assert_eq!(
            db.get_agent_account(&account.id).unwrap().unwrap().status,
            AgentAccountStatus::Error
        );
    }

    #[tokio::test]
    async fn disconnect_reports_revoke_and_credential_store_partial_failures() {
        let (db, store, state, provider, account) = connected_fixture().await;
        *provider.revoke_failure.lock().expect("failure") = Some(AgentProviderError::new(
            "provider_unavailable",
            AgentProviderFailureKind::Retryable,
        ));
        state.disconnect_account(&db, &account.id, false).await.unwrap_err();
        assert_eq!(
            db.get_agent_account(&account.id).unwrap().unwrap().status,
            AgentAccountStatus::DisconnectPending
        );

        *provider.revoke_failure.lock().expect("failure") = None;
        store.fail_next_delete(AgentCredentialStoreError::Locked);
        state.disconnect_account(&db, &account.id, false).await.unwrap_err();
        let pending = db.get_agent_account(&account.id).unwrap().unwrap();
        assert_eq!(pending.status, AgentAccountStatus::DisconnectPending);
        assert_eq!(pending.last_error_code.as_deref(), Some("credential_store_locked"));

        state.disconnect_account(&db, &account.id, false).await.expect("retry disconnect");
        assert!(db.get_agent_account(&account.id).unwrap().is_none());
    }

    /// H3: metadata-only cleanup removes the OS credential first, because the
    /// credential reference lives only on the row being deleted.
    #[tokio::test]
    async fn metadata_only_disconnect_removes_the_credential_before_the_row() {
        let (db, store, state, _provider, account) = connected_fixture().await;
        let credential_ref = db
            .get_agent_account(&account.id)
            .unwrap()
            .unwrap()
            .credential_ref;
        assert!(store.get(&credential_ref).is_ok());

        state
            .disconnect_account(&db, &account.id, true)
            .await
            .expect("metadata-only disconnect");

        assert!(db.get_agent_account(&account.id).unwrap().is_none());
        assert_eq!(
            store.get(&credential_ref).unwrap_err(),
            AgentCredentialStoreError::Missing,
            "the secret was orphaned in the credential store"
        );
    }

    /// H3: an already-missing credential must not block local cleanup.
    #[tokio::test]
    async fn metadata_only_disconnect_tolerates_a_missing_credential() {
        let (db, store, state, _provider, account) = connected_fixture().await;
        let credential_ref = db
            .get_agent_account(&account.id)
            .unwrap()
            .unwrap()
            .credential_ref;
        store.delete(&credential_ref).expect("pre-delete");

        state
            .disconnect_account(&db, &account.id, true)
            .await
            .expect("missing credential is tolerated");
        assert!(db.get_agent_account(&account.id).unwrap().is_none());
    }

    /// H3: a real store failure keeps the row so the secret stays reachable.
    #[tokio::test]
    async fn metadata_only_disconnect_keeps_the_row_when_the_store_fails() {
        let (db, store, state, _provider, account) = connected_fixture().await;
        store.fail_next_delete(AgentCredentialStoreError::Locked);
        let error = state
            .disconnect_account(&db, &account.id, true)
            .await
            .unwrap_err();
        assert_eq!(error.code, "credential_store_locked");
        let pending = db.get_agent_account(&account.id).unwrap().unwrap();
        assert_eq!(pending.status, AgentAccountStatus::DisconnectPending);
        assert!(
            store.get(&pending.credential_ref).is_ok(),
            "the credential must remain reachable through its row"
        );
    }

    /// M1: a connected account past its expiry reports and persists `expired`.
    #[tokio::test]
    async fn past_expiry_is_reported_and_persisted_as_expired() {
        let (db, _store, state, _provider, account) = connected_fixture().await;
        let past = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        db.set_agent_account_state(&account.id, AgentAccountStatus::Connected, Some(&past), None)
            .expect("set expiry");

        let listed = state.list_accounts(&db).expect("list");
        let projected = listed
            .iter()
            .find(|item| item.id == account.id)
            .expect("account");
        assert_eq!(projected.status, AgentAccountStatus::Expired);
        assert_eq!(projected.last_error_code.as_deref(), Some("credential_expired"));

        // The honest state is durable, not just a view-time projection.
        assert_eq!(
            db.get_agent_account(&account.id).unwrap().unwrap().status,
            AgentAccountStatus::Expired
        );
        assert_eq!(
            state
                .get_account(&db, &account.id)
                .expect("get")
                .expect("account")
                .status,
            AgentAccountStatus::Expired
        );
    }

    /// M1: a future or absent expiry leaves a connected account alone.
    #[tokio::test]
    async fn a_future_or_absent_expiry_stays_connected() {
        let (db, _store, state, _provider, account) = connected_fixture().await;
        for expiry in [
            None,
            Some((chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
            // An unparsable provider value must not lock the user out.
            Some("not-a-timestamp".to_string()),
        ] {
            db.set_agent_account_state(
                &account.id,
                AgentAccountStatus::Connected,
                expiry.as_deref(),
                None,
            )
            .expect("set expiry");
            assert_eq!(
                state
                    .get_account(&db, &account.id)
                    .expect("get")
                    .expect("account")
                    .status,
                AgentAccountStatus::Connected,
                "expiry {expiry:?} must stay connected"
            );
        }
    }

    /// B3: the service refuses a completion whose state is missing or wrong.
    #[tokio::test]
    async fn completion_requires_the_exact_authorization_state() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let state = AgentAccountsState::new(store);
        let provider = Arc::new(FakeProvider::new());
        *provider.expected_state.lock().expect("state lock") = None;
        state.register(provider.clone()).expect("register");
        let missing = state
            .start_authorization("codex", AgentHarness::Alfred)
            .unwrap_err();
        assert_eq!(missing.code, "authorization_state_required");

        *provider.expected_state.lock().expect("state lock") = Some(OAUTH_STATE.into());

        for wrong in [None, Some(""), Some("state"), Some("STATE-FIXTURE")] {
            let started = state
                .start_authorization("codex", AgentHarness::Alfred)
                .expect("start");
            let error = state
                .complete_authorization(
                    &db,
                    &started.attempt_id,
                    "codex",
                    AgentHarness::Alfred,
                    wrong,
                )
                .await
                .unwrap_err();
            assert_eq!(
                error.code, "authorization_state_mismatch",
                "state {wrong:?} must not complete"
            );
            assert!(
                db.list_agent_accounts().expect("list").is_empty(),
                "a rejected completion must not create an account"
            );
        }

        let started = state
            .start_authorization("codex", AgentHarness::Alfred)
            .expect("start");
        let account = state
            .complete_authorization(
                &db,
                &started.attempt_id,
                "codex",
                AgentHarness::Alfred,
                Some(OAUTH_STATE),
            )
            .await
            .expect("exact state completes");
        assert_eq!(account.status, AgentAccountStatus::Connected);
    }

    /// Low: per-account mutexes do not accumulate for the process lifetime.
    #[tokio::test]
    async fn account_locks_are_released_when_an_account_is_removed() {
        let (db, _store, state, _provider, account) = connected_fixture().await;
        state
            .refresh_account(&db, &account.id)
            .await
            .expect("refresh takes the lock");
        assert_eq!(state.account_locks.lock().expect("locks").len(), 1);
        state
            .disconnect_account(&db, &account.id, false)
            .await
            .expect("disconnect");
        assert!(
            state.account_locks.lock().expect("locks").is_empty(),
            "the per-account mutex outlived its account"
        );
    }

    #[tokio::test]
    async fn account_lock_map_prunes_idle_entries_and_never_exceeds_its_bound() {
        let state = AgentAccountsState::new(Arc::new(InMemoryAgentCredentialStore::default()));
        let mut active = Vec::new();
        for index in 0..MAX_TRACKED_ACCOUNT_LOCKS {
            active.push(
                state
                    .lock_account(&format!("active-{index}"))
                    .await
                    .expect("lock within bound"),
            );
        }
        let overflow = state.lock_account("overflow").await.unwrap_err();
        assert_eq!(overflow.code, "account_operation_busy");
        assert_eq!(
            state.account_locks.lock().expect("locks").len(),
            MAX_TRACKED_ACCOUNT_LOCKS
        );

        drop(active);
        for index in MAX_TRACKED_ACCOUNT_LOCKS..(MAX_TRACKED_ACCOUNT_LOCKS * 2 + 8) {
            drop(
                state
                    .lock_account(&format!("idle-{index}"))
                    .await
                    .expect("idle locks are pruned"),
            );
            assert!(
                state.account_locks.lock().expect("locks").len()
                    <= MAX_TRACKED_ACCOUNT_LOCKS
            );
        }
    }
}
