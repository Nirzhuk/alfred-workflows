//! Production bridge from Plan 031 accounts to the Plan 032 native runtime.
//!
//! This is the only place a stored credential becomes a live native credential.
//! Provider plans consume it unchanged: they never read the credential store,
//! the account table, or the OS keychain themselves.

use super::credential_store::{
    AgentCredentialEnvelope, AgentCredentialStore, AgentCredentialStoreError,
};
use super::models::{AgentAccountStatus, AgentProductId, CredentialCustodyMode, ManagedRuntimeId};
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
    envelope: Option<AgentCredentialEnvelope>,
    custody_mode: CredentialCustodyMode,
    managed_runtime_id: Option<ManagedRuntimeId>,
    managed_runtime_version: Option<String>,
    runtime_profile_ref: Option<String>,
}

#[allow(dead_code)] // Provider plans read these fields inside their runtime.
impl NativeAgentCredential {
    pub fn access_token(&self) -> Option<&str> {
        self.envelope.as_ref()?.access_token.as_deref()
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.envelope.as_ref()?.refresh_token.as_deref()
    }

    pub fn runtime_credential_ref(&self) -> Option<&str> {
        self.envelope.as_ref()?.runtime_credential_ref.as_deref()
    }

    pub fn provider_field(&self, key: &str) -> Option<&str> {
        self.envelope
            .as_ref()?
            .provider_fields
            .get(key)
            .map(String::as_str)
    }

    pub fn custody_mode(&self) -> super::models::CredentialCustodyMode {
        self.custody_mode
    }

    pub fn expires_at(&self) -> Option<&str> {
        self.envelope.as_ref()?.expires_at.as_deref()
    }

    pub fn managed_runtime_id(&self) -> Option<ManagedRuntimeId> {
        self.managed_runtime_id
    }

    pub fn managed_runtime_version(&self) -> Option<&str> {
        self.managed_runtime_version.as_deref()
    }

    pub fn runtime_profile_ref(&self) -> Option<&str> {
        self.runtime_profile_ref.as_deref()
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
        product: AgentProductId,
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
        if account.provider != provider || account.product != product {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "native account belongs to a different provider product",
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

        let envelope = account
            .credential_ref
            .as_deref()
            .map(|credential_ref| self.credential_store.get(credential_ref))
            .transpose()
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
            product: account.product,
            credential: NativeCredential::new(NativeAgentCredential {
                envelope,
                custody_mode: account.custody_mode,
                managed_runtime_id: account.managed_runtime_id,
                managed_runtime_version: account.managed_runtime_version,
                runtime_profile_ref: account.runtime_profile_ref,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::credential_store::InMemoryAgentCredentialStore;
    use crate::agent_accounts::models::{AgentEntitlementState, AuthorizedAgentAccount};
    use crate::agents::AgentHarness;
    use chrono::{Duration as ChronoDuration, Utc};

    fn grant(product: AgentProductId, account: &str) -> AuthorizedAgentAccount {
        AuthorizedAgentAccount {
            provider: product.provider(),
            product,
            harness: AgentHarness::Alfred,
            display_name: Some(account.into()),
            external_account_id: account.into(),
            external_workspace_id: None,
            auth_method: product.auth_method(),
            custody_mode: product.custody_mode(),
            managed_runtime_id: product.managed_runtime(),
            managed_runtime_version: product.managed_runtime_version().map(str::to_owned),
            runtime_profile_ref: product
                .managed_runtime()
                .map(|_| format!("profile-{account}")),
            scopes: vec!["models:read".into()],
            billing_source: product.billing_source().into(),
            billing_owner: product.billing_owner().into(),
            entitlement_state: AgentEntitlementState::Unknown,
            entitlement_source: "provider_unobserved".into(),
            entitlement_observed_at: None,
            expires_at: None,
        }
    }

    struct Fixture {
        resolver: AgentAccountResolver,
        db: Arc<Db>,
        store: Arc<InMemoryAgentCredentialStore>,
    }

    fn fixture(
        product: AgentProductId,
        status: AgentAccountStatus,
        expires_at: Option<&str>,
    ) -> (Fixture, OpaqueAgentAccountRef) {
        let db = Arc::new(Db::open_in_memory().expect("database"));
        let store = Arc::new(InMemoryAgentCredentialStore::default());
        let account = db
            .prepare_agent_account(grant(product, "user"))
            .expect("account");
        if let Some(credential_ref) = account.credential_ref.as_deref() {
            store
                .put(
                    credential_ref,
                    &AgentCredentialEnvelope::alfred_managed("access-secret-fixture".into()),
                )
                .expect("credential");
        }
        db.set_agent_account_state(&account.id, status, expires_at, None)
            .expect("state");
        let account_ref = OpaqueAgentAccountRef::parse(&account.id).expect("ref");
        let resolver = AgentAccountResolver {
            db: Arc::clone(&db),
            credential_store: store.clone(),
        };
        (
            Fixture {
                resolver,
                db,
                store,
            },
            account_ref,
        )
    }

    #[test]
    fn connected_account_resolves_the_secret_from_the_credential_store() {
        let (fixture, account_ref) = fixture(
            AgentProductId::OpenaiApi,
            AgentAccountStatus::Connected,
            None,
        );
        let resolved = fixture
            .resolver
            .resolve(
                &account_ref,
                AgentProvider::Codex,
                AgentProductId::OpenaiApi,
            )
            .expect("resolve");
        assert_eq!(resolved.account_ref, account_ref);
        assert_eq!(resolved.provider, AgentProvider::Codex);
        assert_eq!(resolved.product, AgentProductId::OpenaiApi);
        let credential = resolved
            .credential
            .downcast_ref::<NativeAgentCredential>()
            .expect("native credential");
        assert_eq!(credential.access_token(), Some("access-secret-fixture"));
        assert_eq!(
            credential.custody_mode(),
            CredentialCustodyMode::AlfredManaged
        );
        assert_eq!(credential.runtime_profile_ref(), None);
        // The credential never renders its secret.
        assert!(!format!("{:?}", resolved.credential).contains("access-secret-fixture"));
        assert!(!format!("{resolved:?}").contains("access-secret-fixture"));
    }

    #[test]
    fn provider_mismatch_and_unknown_reference_are_refused() {
        let (fixture, account_ref) = fixture(
            AgentProductId::OpenaiApi,
            AgentAccountStatus::Connected,
            None,
        );
        assert_eq!(
            fixture
                .resolver
                .resolve(
                    &account_ref,
                    AgentProvider::Gemini,
                    AgentProductId::GeminiApi,
                )
                .unwrap_err()
                .code,
            NativeErrorCode::AccountMismatch
        );
        assert_eq!(
            fixture
                .resolver
                .resolve(
                    &account_ref,
                    AgentProvider::Codex,
                    AgentProductId::ChatgptCodex,
                )
                .unwrap_err()
                .code,
            NativeErrorCode::AccountMismatch
        );
        let unknown = OpaqueAgentAccountRef::parse("account_missing-01").expect("ref");
        assert_eq!(
            fixture
                .resolver
                .resolve(&unknown, AgentProvider::Codex, AgentProductId::OpenaiApi)
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
            let (fixture, account_ref) = fixture(AgentProductId::OpenaiApi, status, None);
            assert_eq!(
                fixture
                    .resolver
                    .resolve(
                        &account_ref,
                        AgentProvider::Codex,
                        AgentProductId::OpenaiApi,
                    )
                    .unwrap_err()
                    .code,
                NativeErrorCode::AccountUnavailable,
                "status {status:?} must not resolve"
            );
        }

        let past = (Utc::now() - ChronoDuration::minutes(5)).to_rfc3339();
        let (fixture, account_ref) = fixture(
            AgentProductId::OpenaiApi,
            AgentAccountStatus::Connected,
            Some(&past),
        );
        assert_eq!(
            fixture
                .resolver
                .resolve(
                    &account_ref,
                    AgentProvider::Codex,
                    AgentProductId::OpenaiApi,
                )
                .unwrap_err()
                .code,
            NativeErrorCode::AccountUnavailable
        );
    }

    #[test]
    fn a_missing_credential_is_reported_without_leaking_the_reference() {
        let (fixture, account_ref) = fixture(
            AgentProductId::OpenaiApi,
            AgentAccountStatus::Connected,
            None,
        );
        let account = fixture
            .db
            .get_agent_account(account_ref.as_str())
            .expect("read")
            .expect("account");
        let credential_ref = account.credential_ref.as_deref().expect("credential ref");
        fixture.store.delete(credential_ref).expect("delete");
        let error = fixture
            .resolver
            .resolve(
                &account_ref,
                AgentProvider::Codex,
                AgentProductId::OpenaiApi,
            )
            .unwrap_err();
        assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
        assert!(!error.message.contains(credential_ref));
    }

    #[test]
    fn managed_subscription_resolves_only_through_its_opaque_runtime_profile() {
        let (fixture, account_ref) = fixture(
            AgentProductId::ChatgptCodex,
            AgentAccountStatus::Connected,
            None,
        );
        let account = fixture
            .db
            .get_agent_account(account_ref.as_str())
            .expect("read")
            .expect("account");
        assert!(account.credential_ref.is_none());
        let resolved = fixture
            .resolver
            .resolve(
                &account_ref,
                AgentProvider::Codex,
                AgentProductId::ChatgptCodex,
            )
            .expect("resolve");
        let credential = resolved
            .credential
            .downcast_ref::<NativeAgentCredential>()
            .expect("native credential");
        assert_eq!(credential.access_token(), None);
        assert_eq!(
            credential.managed_runtime_id(),
            Some(ManagedRuntimeId::CodexPythonSdk)
        );
        assert_eq!(credential.managed_runtime_version(), Some("0.147.0"));
        assert_eq!(credential.runtime_profile_ref(), Some("profile-user"));
    }
}
