use super::models::{AgentAuthMethod, AgentProductId};
use crate::agents::{AgentHarness, AgentProvider};
use chrono::{DateTime, Utc};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use zeroize::Zeroize;

const MAX_ACTIVE_ATTEMPTS: usize = 8;
/// Cancellation flags outlive their attempt only until the next insert; this
/// cap stops a long-lived process from accumulating tombstones.
const MAX_CANCELLATION_TOMBSTONES: usize = 64;

pub struct AuthorizationContext {
    pub expected_state: Option<String>,
    pub pkce_verifier: Option<String>,
    pub nonce: Option<String>,
    pub provider_fields: BTreeMap<String, String>,
}

impl AuthorizationContext {
    #[allow(dead_code)] // Provider plans use this for flows without PKCE/state.
    pub fn empty() -> Self {
        Self {
            expected_state: None,
            pkce_verifier: None,
            nonce: None,
            provider_fields: BTreeMap::new(),
        }
    }
}

impl fmt::Debug for AuthorizationContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationContext")
            .field(
                "expected_state",
                &self.expected_state.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "pkce_verifier",
                &self.pkce_verifier.as_ref().map(|_| "[REDACTED]"),
            )
            .field("nonce", &self.nonce.as_ref().map(|_| "[REDACTED]"))
            .field("provider_fields", &"[REDACTED]")
            .finish()
    }
}

impl Drop for AuthorizationContext {
    fn drop(&mut self) {
        self.expected_state.zeroize();
        self.pkce_verifier.zeroize();
        self.nonce.zeroize();
        for value in self.provider_fields.values_mut() {
            value.zeroize();
        }
    }
}

pub struct AuthorizationAttempt {
    pub id: String,
    pub provider: AgentProvider,
    pub product: AgentProductId,
    pub harness: AgentHarness,
    pub auth_method: AgentAuthMethod,
    pub expires_at: DateTime<Utc>,
    pub cancelled: Arc<AtomicBool>,
    pub context: AuthorizationContext,
}

impl fmt::Debug for AuthorizationAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationAttempt")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("product", &self.product)
            .field("harness", &self.harness)
            .field("auth_method", &self.auth_method)
            .field("expires_at", &self.expires_at)
            .field("cancelled", &self.cancelled.load(Ordering::SeqCst))
            .field("context", &self.context)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStartedDto {
    pub attempt_id: String,
    pub provider_id: String,
    pub product_id: String,
    pub authorization_url: Option<String>,
    pub user_code: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationRegistryError {
    #[error("authorization attempt was not found")]
    NotFound,
    #[error("authorization attempt expired")]
    Expired,
    #[error("authorization attempt was cancelled")]
    Cancelled,
    #[error("authorization state did not match")]
    StateMismatch,
    #[error("OAuth PKCE authorization requires state")]
    StateRequired,
    #[error("authorization provider did not match")]
    ProviderMismatch,
    #[error("too many authorization attempts are active")]
    Busy,
    #[error("authorization registry is unavailable")]
    Lock,
}

pub struct AuthorizationAttemptRegistry {
    attempts: Mutex<HashMap<String, AuthorizationAttempt>>,
    cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Default for AuthorizationAttemptRegistry {
    fn default() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
        }
    }
}

impl AuthorizationAttemptRegistry {
    pub fn insert(
        &self,
        provider: AgentProvider,
        product: AgentProductId,
        harness: AgentHarness,
        auth_method: AgentAuthMethod,
        ttl: Duration,
        context: AuthorizationContext,
    ) -> Result<(String, DateTime<Utc>), AuthorizationRegistryError> {
        if product.provider() != provider || !product.auth_methods().contains(&auth_method.as_str())
        {
            return Err(AuthorizationRegistryError::ProviderMismatch);
        }
        if auth_method == AgentAuthMethod::OAuthPkce
            && context
                .expected_state
                .as_deref()
                .is_none_or(|state| state.trim().is_empty())
        {
            return Err(AuthorizationRegistryError::StateRequired);
        }
        let now = Utc::now();
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| AuthorizationRegistryError::Lock)?;
        let expired_ids = attempts
            .iter()
            .filter(|(_, attempt)| {
                attempt.expires_at <= now || attempt.cancelled.load(Ordering::SeqCst)
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &expired_ids {
            attempts.remove(id);
        }
        if !expired_ids.is_empty() {
            let mut cancellations = self
                .cancellations
                .lock()
                .map_err(|_| AuthorizationRegistryError::Lock)?;
            for id in expired_ids {
                cancellations.remove(&id);
            }
        }
        if attempts.len() >= MAX_ACTIVE_ATTEMPTS {
            return Err(AuthorizationRegistryError::Busy);
        }
        {
            let mut cancellations = self
                .cancellations
                .lock()
                .map_err(|_| AuthorizationRegistryError::Lock)?;
            cancellations.retain(|id, _| attempts.contains_key(id));
            if cancellations.len() > MAX_CANCELLATION_TOMBSTONES {
                cancellations.clear();
            }
        }
        let id: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();
        let expires_at =
            now + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::minutes(10));
        let cancelled = Arc::new(AtomicBool::new(false));
        attempts.insert(
            id.clone(),
            AuthorizationAttempt {
                id: id.clone(),
                provider,
                product,
                harness,
                auth_method,
                expires_at,
                cancelled: cancelled.clone(),
                context,
            },
        );
        self.cancellations
            .lock()
            .map_err(|_| AuthorizationRegistryError::Lock)?
            .insert(id.clone(), cancelled);
        Ok((id, expires_at))
    }

    /// Completes an attempt. When the attempt recorded an expected state, the
    /// caller MUST present exactly that state; a missing state is a mismatch,
    /// never a bypass.
    pub fn take(
        &self,
        id: &str,
        provider: AgentProvider,
        product: AgentProductId,
        harness: AgentHarness,
        completion_state: Option<&str>,
    ) -> Result<AuthorizationAttempt, AuthorizationRegistryError> {
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| AuthorizationRegistryError::Lock)?;
        let attempt = attempts
            .remove(id)
            .ok_or(AuthorizationRegistryError::NotFound)?;
        if attempt.cancelled.load(Ordering::SeqCst) {
            self.finish(id);
            return Err(AuthorizationRegistryError::Cancelled);
        }
        if attempt.expires_at <= Utc::now() {
            self.finish(id);
            return Err(AuthorizationRegistryError::Expired);
        }
        if attempt.provider != provider || attempt.product != product || attempt.harness != harness
        {
            self.finish(id);
            return Err(AuthorizationRegistryError::ProviderMismatch);
        }
        if attempt.auth_method == AgentAuthMethod::OAuthPkce
            && attempt
                .context
                .expected_state
                .as_deref()
                .is_none_or(|state| state.trim().is_empty())
        {
            self.finish(id);
            return Err(AuthorizationRegistryError::StateRequired);
        }
        if let Some(expected) = attempt.context.expected_state.as_deref() {
            // A present expected state requires an exact match. `None` is a
            // mismatch so no caller can complete a flow without the callback.
            if completion_state != Some(expected) {
                self.finish(id);
                return Err(AuthorizationRegistryError::StateMismatch);
            }
        } else if completion_state.is_some() {
            self.finish(id);
            return Err(AuthorizationRegistryError::StateMismatch);
        }
        Ok(attempt)
    }

    pub fn cancel(&self, id: &str) -> Result<(), AuthorizationRegistryError> {
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| AuthorizationRegistryError::Lock)?;
        let attempt = attempts.remove(id);
        let cancelled = self
            .cancellations
            .lock()
            .map_err(|_| AuthorizationRegistryError::Lock)?
            .remove(id)
            .or_else(|| attempt.as_ref().map(|attempt| attempt.cancelled.clone()))
            .ok_or(AuthorizationRegistryError::NotFound)?;
        cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn finish(&self, id: &str) {
        if let Ok(mut cancellations) = self.cancellations.lock() {
            cancellations.remove(id);
        }
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.attempts.lock().expect("attempt lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = "state-secret";
    const PRODUCT: AgentProductId = AgentProductId::ChatgptCodex;

    fn context() -> AuthorizationContext {
        AuthorizationContext {
            expected_state: Some(STATE.into()),
            pkce_verifier: Some("verifier-secret".into()),
            nonce: Some("nonce-secret".into()),
            provider_fields: BTreeMap::from([("device".into(), "device-secret".into())]),
        }
    }

    fn stateless_context() -> AuthorizationContext {
        AuthorizationContext::empty()
    }

    fn insert(registry: &AuthorizationAttemptRegistry, ttl: Duration) -> String {
        registry
            .insert(
                AgentProvider::Codex,
                PRODUCT,
                AgentHarness::Alfred,
                AgentAuthMethod::OAuthPkce,
                ttl,
                context(),
            )
            .expect("insert")
            .0
    }

    #[test]
    fn timeout_cancellation_duplicate_provider_and_state_mismatch_are_safe() {
        let registry = AuthorizationAttemptRegistry::default();
        let expired = insert(&registry, Duration::ZERO);
        assert_eq!(
            registry
                .take(
                    &expired,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some(STATE)
                )
                .unwrap_err(),
            AuthorizationRegistryError::Expired
        );

        let cancelled = insert(&registry, Duration::from_secs(30));
        registry.cancel(&cancelled).expect("cancel");
        assert_eq!(registry.active_count(), 0);
        assert_eq!(
            registry
                .take(
                    &cancelled,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some(STATE)
                )
                .unwrap_err(),
            AuthorizationRegistryError::NotFound
        );

        let mismatch = insert(&registry, Duration::from_secs(30));
        assert_eq!(
            registry
                .take(
                    &mismatch,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some("wrong")
                )
                .unwrap_err(),
            AuthorizationRegistryError::StateMismatch
        );

        let provider = insert(&registry, Duration::from_secs(30));
        assert_eq!(
            registry
                .take(
                    &provider,
                    AgentProvider::Gemini,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some(STATE)
                )
                .unwrap_err(),
            AuthorizationRegistryError::ProviderMismatch
        );

        let once = insert(&registry, Duration::from_secs(30));
        registry
            .take(
                &once,
                AgentProvider::Codex,
                PRODUCT,
                AgentHarness::Alfred,
                Some(STATE),
            )
            .expect("first completion");
        assert_eq!(
            registry
                .take(
                    &once,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some(STATE)
                )
                .unwrap_err(),
            AuthorizationRegistryError::NotFound
        );

        let active = insert(&registry, Duration::from_secs(30));
        let active_attempt = registry
            .take(
                &active,
                AgentProvider::Codex,
                PRODUCT,
                AgentHarness::Alfred,
                Some(STATE),
            )
            .expect("take active");
        registry.cancel(&active).expect("cancel active completion");
        assert!(active_attempt.cancelled.load(Ordering::SeqCst));
    }

    /// B3: OAuth PKCE attempts cannot start without non-empty state, and a
    /// recorded state must be presented exactly at completion.
    #[test]
    fn oauth_state_is_mandatory_and_exact() {
        let registry = AuthorizationAttemptRegistry::default();

        for expected_state in [None, Some(String::new()), Some("   ".into())] {
            assert_eq!(
                registry
                    .insert(
                        AgentProvider::Codex,
                        PRODUCT,
                        AgentHarness::Alfred,
                        AgentAuthMethod::OAuthPkce,
                        Duration::from_secs(30),
                        AuthorizationContext {
                            expected_state,
                            pkce_verifier: Some("verifier-secret".into()),
                            nonce: None,
                            provider_fields: BTreeMap::new(),
                        },
                    )
                    .unwrap_err(),
                AuthorizationRegistryError::StateRequired
            );
        }

        let missing = insert(&registry, Duration::from_secs(30));
        assert_eq!(
            registry
                .take(
                    &missing,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    None
                )
                .unwrap_err(),
            AuthorizationRegistryError::StateMismatch
        );

        let empty = insert(&registry, Duration::from_secs(30));
        assert_eq!(
            registry
                .take(
                    &empty,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some("")
                )
                .unwrap_err(),
            AuthorizationRegistryError::StateMismatch
        );

        let prefix = insert(&registry, Duration::from_secs(30));
        assert_eq!(
            registry
                .take(
                    &prefix,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some("state")
                )
                .unwrap_err(),
            AuthorizationRegistryError::StateMismatch
        );

        let exact = insert(&registry, Duration::from_secs(30));
        registry
            .take(
                &exact,
                AgentProvider::Codex,
                PRODUCT,
                AgentHarness::Alfred,
                Some(STATE),
            )
            .expect("exact state completes");

        // Runtime and device-code contracts may remain state-less, but still
        // reject an unexpected callback state they did not record.
        for (provider, product, auth_method) in [
            (
                AgentProvider::ClaudeCode,
                AgentProductId::ClaudeCodeSubscription,
                AgentAuthMethod::Runtime,
            ),
            (AgentProvider::Codex, PRODUCT, AgentAuthMethod::DeviceCode),
        ] {
            let (stateless, _) = registry
                .insert(
                    provider,
                    product,
                    AgentHarness::Alfred,
                    auth_method,
                    Duration::from_secs(30),
                    stateless_context(),
                )
                .expect("insert stateless");
            assert_eq!(
                registry
                    .take(
                        &stateless,
                        provider,
                        product,
                        AgentHarness::Alfred,
                        Some("unexpected"),
                    )
                    .unwrap_err(),
                AuthorizationRegistryError::StateMismatch
            );

            let (stateless_ok, _) = registry
                .insert(
                    provider,
                    product,
                    AgentHarness::Alfred,
                    auth_method,
                    Duration::from_secs(30),
                    stateless_context(),
                )
                .expect("insert stateless");
            registry
                .take(&stateless_ok, provider, product, AgentHarness::Alfred, None)
                .expect("stateless flow completes without a state");
        }
    }

    #[test]
    fn restart_drops_all_attempts_and_debug_is_redacted() {
        let registry = AuthorizationAttemptRegistry::default();
        let id = insert(&registry, Duration::from_secs(30));
        let attempt = registry
            .take(
                &id,
                AgentProvider::Codex,
                PRODUCT,
                AgentHarness::Alfred,
                Some(STATE),
            )
            .expect("take");
        let output = format!("{attempt:?}");
        for secret in [
            "state-secret",
            "verifier-secret",
            "nonce-secret",
            "device-secret",
        ] {
            assert!(!output.contains(secret));
        }
        let restarted = AuthorizationAttemptRegistry::default();
        assert_eq!(
            restarted
                .take(
                    &id,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some(STATE)
                )
                .unwrap_err(),
            AuthorizationRegistryError::NotFound
        );
    }

    /// Low: cancellation tombstones are pruned instead of accumulating.
    #[test]
    fn cancellation_tombstones_stay_bounded() {
        let registry = AuthorizationAttemptRegistry::default();
        for _ in 0..64 {
            let id = insert(&registry, Duration::from_secs(30));
            registry
                .take(
                    &id,
                    AgentProvider::Codex,
                    PRODUCT,
                    AgentHarness::Alfred,
                    Some(STATE),
                )
                .expect("complete");
        }
        // One more insert prunes every tombstone whose attempt is gone.
        let live = insert(&registry, Duration::from_secs(30));
        let tombstones = registry.cancellations.lock().expect("lock").len();
        assert!(tombstones <= 1, "tombstones leaked: {tombstones}");
        assert!(registry.active_count() <= MAX_ACTIVE_ATTEMPTS);
        registry.cancel(&live).expect("cancel");
    }
}
