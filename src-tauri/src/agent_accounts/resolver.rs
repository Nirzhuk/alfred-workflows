//! Production bridge from Plan 031 accounts to the Plan 032 native runtime.
//!
//! This is the only place a stored credential becomes a live native credential.
//! Provider plans consume it unchanged: they never read the credential store,
//! the account table, or the OS keychain themselves.

use super::credential_store::{
    AgentCredentialEnvelope, AgentCredentialStore, AgentCredentialStoreError,
};
use super::models::AgentAccountStatus;
use super::service::AgentAccountsState;
use crate::agents::native::{
    NativeAccountResolver, NativeCredential, NativeErrorCode, NativeRuntimeError,
    ResolvedNativeAccount,
};
use crate::agents::{AgentProvider, OpaqueAgentAccountRef};
use crate::db::Db;
use std::sync::Arc;

/// The live credential handed to a native runtime.
///
/// It is deliberately not `Serialize`, not `Debug`-printable, and never leaves
/// the runtime call; `NativeCredential` erases it behind a redacted `Debug`.
pub struct NativeAgentCredential {
    envelope: AgentCredentialEnvelope,
}

#[allow(dead_code)] // Provider plans read these fields inside their runtime.
impl NativeAgentCredential {
    pub fn access_token(&self) -> Option<&str> {
        self.envelope.access_token.as_deref()
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.envelope.refresh_token.as_deref()
    }

    pub fn runtime_credential_ref(&self) -> Option<&str> {
        self.envelope.runtime_credential_ref.as_deref()
    }

    pub fn provider_field(&self, key: &str) -> Option<&str> {
        self.envelope.provider_fields.get(key).map(String::as_str)
    }

    pub fn custody_mode(&self) -> super::models::CredentialCustodyMode {
        self.envelope.custody_mode
    }

    pub fn expires_at(&self) -> Option<&str> {
        self.envelope.expires_at.as_deref()
    }
}

/// Resolves an opaque account reference into a validated, credentialed account.
pub struct AgentAccountResolver {
    db: Arc<Db>,
    credential_store: Arc<dyn AgentCredentialStore>,
}

impl AgentAccountResolver {
    pub fn new(db: Arc<Db>, accounts: &AgentAccountsState) -> Self {
        Self {
            db,
            credential_store: accounts.credential_store(),
        }
    }
}

impl NativeAccountResolver for AgentAccountResolver {
    fn resolve(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        provider: AgentProvider,
    ) -> Result<ResolvedNativeAccount, NativeRuntimeError> {
        let account = self
            .db
            .get_agent_account(account_ref.as_str())
            .map_err(|_| {
                NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "native account metadata could not be read",
                    true,
                )
            })?
            .ok_or_else(|| {
                NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "native account no longer exists",
                    false,
                )
            })?;

        // A workflow node cannot borrow another provider's account.
        if account.provider != provider {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "native account belongs to a different provider",
                false,
            ));
        }
        if account.harness != crate::agents::AgentHarness::Alfred {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "native account is not registered for the Alfred harness",
                false,
            ));
        }
        // Only a healthy account yields a credential. Expired, error, revoked,
        // and half-disconnected accounts must be repaired in Settings first.
        if account.status != AgentAccountStatus::Connected {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "native account is not connected",
                false,
            ));
        }
        if super::service::is_past_expiry(account.expires_at.as_deref()) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "native account credential expired; refresh or reconnect it",
                false,
            ));
        }

        let envelope = self
            .credential_store
            .get(&account.credential_ref)
            .map_err(|error| match error {
                AgentCredentialStoreError::Missing => NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "native account credential is missing; reconnect the account",
                    false,
                ),
                AgentCredentialStoreError::Locked => NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "system credential store is locked",
                    true,
                ),
                AgentCredentialStoreError::Invalid => NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "native account credential is invalid; reconnect the account",
                    false,
                ),
                AgentCredentialStoreError::Failed => NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "native account credential could not be read",
                    true,
                ),
            })?;

        Ok(ResolvedNativeAccount {
            account_ref: account_ref.clone(),
            provider,
            credential: NativeCredential::new(NativeAgentCredential { envelope }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::credential_store::InMemoryAgentCredentialStore;
    use crate::agent_accounts::models::{
        AgentAuthMethod, AuthorizedAgentAccount, CredentialCustodyMode,
    };
    use crate::agents::AgentHarness;
    use chrono::{Duration as ChronoDuration, Utc};

    fn grant(provider: AgentProvider, account: &str) -> AuthorizedAgentAccount {
        AuthorizedAgentAccount {
            provider,
            harness: AgentHarness::Alfred,
            display_name: Some(account.into()),
            external_account_id: account.into(),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::OAuthPkce,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            scopes: vec!["models:read".into()],
            expires_at: None,
        }
    }

    struct Fixture {
        resolver: AgentAccountResolver,
        db: Arc<Db>,
        store: Arc<InMemoryAgentCredentialStore>,
    }

    fn fixture(provider: AgentProvider, status: AgentAccountStatus, expires_at: Option<&str>) -> (Fixture, OpaqueAgentAccountRef) {
        let db = Arc::new(Db::open_in_memory().expect("database"));
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let account = db.prepare_agent_account(grant(provider, "user")).expect("account");
        store
            .put(
                &account.credential_ref,
                &AgentCredentialEnvelope::alfred_managed("access-secret-fixture".into()),
            )
            .expect("credential");
        db.set_agent_account_state(&account.id, status, expires_at, None)
            .expect("state");
        let account_ref = OpaqueAgentAccountRef::parse(&account.id).expect("ref");
        let resolver = AgentAccountResolver {
            db: Arc::clone(&db),
            credential_store: store.clone(),
        };
        (Fixture { resolver, db, store }, account_ref)
    }

    #[test]
    fn connected_account_resolves_the_secret_from_the_credential_store() {
        let (fixture, account_ref) = fixture(AgentProvider::Codex, AgentAccountStatus::Connected, None);
        let resolved = fixture
            .resolver
            .resolve(&account_ref, AgentProvider::Codex)
            .expect("resolve");
        assert_eq!(resolved.account_ref, account_ref);
        assert_eq!(resolved.provider, AgentProvider::Codex);
        let credential = resolved
            .credential
            .downcast_ref::<NativeAgentCredential>()
            .expect("native credential");
        assert_eq!(credential.access_token(), Some("access-secret-fixture"));
        assert_eq!(
            credential.custody_mode(),
            CredentialCustodyMode::AlfredManaged
        );
        // The credential never renders its secret.
        assert!(!format!("{:?}", resolved.credential).contains("access-secret-fixture"));
        assert!(!format!("{resolved:?}").contains("access-secret-fixture"));
    }

    #[test]
    fn provider_mismatch_and_unknown_reference_are_refused() {
        let (fixture, account_ref) = fixture(AgentProvider::Codex, AgentAccountStatus::Connected, None);
        assert_eq!(
            fixture
                .resolver
                .resolve(&account_ref, AgentProvider::Gemini)
                .unwrap_err()
                .code,
            NativeErrorCode::AccountMismatch
        );
        let unknown = OpaqueAgentAccountRef::parse("account_missing-01").expect("ref");
        assert_eq!(
            fixture
                .resolver
                .resolve(&unknown, AgentProvider::Codex)
                .unwrap_err()
                .code,
            NativeErrorCode::AccountUnavailable
        );
    }

    #[test]
    fn non_connected_and_expired_accounts_never_yield_a_credential() {
        for status in [
            AgentAccountStatus::Error,
            AgentAccountStatus::Expired,
            AgentAccountStatus::Revoked,
            AgentAccountStatus::DisconnectPending,
        ] {
            let (fixture, account_ref) = fixture(AgentProvider::Codex, status, None);
            assert_eq!(
                fixture
                    .resolver
                    .resolve(&account_ref, AgentProvider::Codex)
                    .unwrap_err()
                    .code,
                NativeErrorCode::AccountUnavailable,
                "status {status:?} must not resolve"
            );
        }

        let past = (Utc::now() - ChronoDuration::minutes(5)).to_rfc3339();
        let (fixture, account_ref) =
            fixture(AgentProvider::Codex, AgentAccountStatus::Connected, Some(&past));
        assert_eq!(
            fixture
                .resolver
                .resolve(&account_ref, AgentProvider::Codex)
                .unwrap_err()
                .code,
            NativeErrorCode::AccountUnavailable
        );
    }

    #[test]
    fn a_missing_credential_is_reported_without_leaking_the_reference() {
        let (fixture, account_ref) = fixture(AgentProvider::Codex, AgentAccountStatus::Connected, None);
        let account = fixture
            .db
            .get_agent_account(account_ref.as_str())
            .expect("read")
            .expect("account");
        fixture.store.delete(&account.credential_ref).expect("delete");
        let error = fixture
            .resolver
            .resolve(&account_ref, AgentProvider::Codex)
            .unwrap_err();
        assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
        assert!(!error.message.contains(&account.credential_ref));
    }
}
