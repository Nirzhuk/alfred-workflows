pub mod actions;
pub mod catalog;
pub mod models;
pub mod oauth_native;
pub mod refresh;
pub mod token_store;

use self::actions::ActionRegistry;
use self::actions::{
    ActionCancellation, ActionDescriptor, ActionRequest, ActionResourcePage, ActionResult,
};
use self::catalog::ProviderCatalog;
use self::models::IntegrationCommandError;
use self::refresh::RefreshService;
use self::token_store::{OsTokenStore, TokenStore, TokenStoreError};
use crate::db::Db;
use std::sync::Arc;

pub struct IntegrationsState {
    pub actions: ActionRegistry,
    pub catalog: ProviderCatalog,
    pub refresh: RefreshService,
    token_store: Arc<dyn TokenStore>,
}

impl Default for IntegrationsState {
    fn default() -> Self {
        Self::new(Arc::new(OsTokenStore))
    }
}

impl IntegrationsState {
    pub fn new(token_store: Arc<dyn TokenStore>) -> Self {
        Self {
            actions: ActionRegistry::default(),
            catalog: ProviderCatalog::default(),
            refresh: RefreshService::new(token_store.clone()),
            token_store,
        }
    }

    pub fn action_descriptors(&self, provider_id: Option<&str>) -> Vec<ActionDescriptor> {
        self.actions.descriptors(provider_id)
    }

    pub async fn execute_action(
        &self,
        db: &Db,
        request: ActionRequest,
        cancellation: ActionCancellation,
    ) -> Result<ActionResult, actions::ActionError> {
        self.actions
            .execute(
                db,
                &self.refresh,
                self.token_store.clone(),
                request,
                cancellation,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_action_resources(
        &self,
        db: &Db,
        connection_id: &str,
        provider_id: &str,
        action_id: &str,
        field_key: &str,
        query: &str,
        page_token: Option<&str>,
    ) -> Result<ActionResourcePage, actions::ActionError> {
        self.actions
            .list_resources(
                db,
                self.token_store.clone(),
                connection_id,
                provider_id,
                action_id,
                field_key,
                query,
                page_token,
            )
            .await
    }

    pub async fn disconnect(
        &self,
        db: &Db,
        connection_id: &str,
        metadata_only: bool,
    ) -> Result<(), IntegrationCommandError> {
        let _guard = self
            .refresh
            .lock_connection(connection_id)
            .await
            .map_err(|_| {
                IntegrationCommandError::new(
                    "disconnect_failed",
                    "The connection is busy. Try disconnecting again.",
                    true,
                )
            })?;
        let connection = db
            .get_app_connection(connection_id)
            .map_err(|_| database_error())?
            .ok_or_else(IntegrationCommandError::not_found)?;
        db.mark_app_connection_revoked(connection_id)
            .map_err(|_| database_error())?;

        if !metadata_only {
            let store = self.token_store.clone();
            let credential_ref = connection.credential_ref;
            let deletion =
                tauri::async_runtime::spawn_blocking(move || store.delete(&credential_ref))
                    .await
                    .map_err(|_| {
                        IntegrationCommandError::new(
                            "disconnect_failed",
                            "The credential could not be removed. The connection remains revoked.",
                            true,
                        )
                    })?;
            if let Err(error) = deletion {
                return Err(token_store_command_error(error));
            }
        }

        db.delete_app_connection_metadata(connection_id)
            .map_err(|_| database_error())
    }
}

fn database_error() -> IntegrationCommandError {
    IntegrationCommandError::new(
        "connection_store_failed",
        "Connected-app metadata could not be updated.",
        true,
    )
}

fn token_store_command_error(error: TokenStoreError) -> IntegrationCommandError {
    match error {
        TokenStoreError::Missing => IntegrationCommandError::new(
            "credential_missing",
            "The credential is already missing. You can remove local metadata only.",
            true,
        ),
        TokenStoreError::Locked => IntegrationCommandError::new(
            "credential_store_locked",
            "Unlock the system credential store and try again, or remove local metadata only.",
            true,
        ),
        TokenStoreError::Invalid | TokenStoreError::Failed => IntegrationCommandError::new(
            "disconnect_failed",
            "The credential could not be removed. The connection remains revoked.",
            true,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::models::{canonical_identity_key, UpsertAppConnection};
    use crate::integrations::token_store::{CredentialEnvelope, InMemoryTokenStore};

    #[tokio::test]
    async fn disconnect_deletes_credential_before_metadata() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let state = IntegrationsState::new(store.clone());
        let connection = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "slack".into(),
                display_name: Some("Workspace".into()),
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "native_oauth".into(),
                identity_key: canonical_identity_key("slack", "native_oauth", &["workspace"]),
                scopes: vec![],
                expires_at: None,
                credential_ref: "credential".into(),
            })
            .expect("connection");
        store
            .put("credential", &CredentialEnvelope::new("token".into()))
            .expect("credential");

        state
            .disconnect(&db, &connection.id, false)
            .await
            .expect("disconnect");
        assert_eq!(
            store.get("credential").unwrap_err(),
            TokenStoreError::Missing
        );
        assert!(db
            .get_app_connection(&connection.id)
            .expect("read")
            .is_none());
    }

    #[tokio::test]
    async fn failed_cleanup_leaves_revoked_metadata_for_explicit_recovery() {
        let db = Db::open_in_memory().expect("database");
        let store = Arc::new(InMemoryTokenStore::default());
        let state = IntegrationsState::new(store);
        let connection = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "slack".into(),
                display_name: None,
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "native_oauth".into(),
                identity_key: canonical_identity_key("slack", "native_oauth", &["workspace"]),
                scopes: vec![],
                expires_at: None,
                credential_ref: "missing".into(),
            })
            .expect("connection");

        let error = state
            .disconnect(&db, &connection.id, false)
            .await
            .expect_err("missing credential");
        assert_eq!(error.code, "credential_missing");
        assert_eq!(
            db.get_app_connection(&connection.id)
                .expect("read")
                .expect("connection")
                .status,
            crate::integrations::models::ConnectionStatus::Revoked
        );

        state
            .disconnect(&db, &connection.id, true)
            .await
            .expect("metadata cleanup");
        assert!(db
            .get_app_connection(&connection.id)
            .expect("read")
            .is_none());
    }
}
