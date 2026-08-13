use crate::db::Db;
use crate::integrations::actions::{ActionDescriptor, ActionError, ActionResourcePage};
use crate::integrations::models::{
    AppConnectionDto, AppConnectionUsage, AppProviderDto, IntegrationCommandError,
};
use crate::integrations::IntegrationsState;
use tauri::State;

#[tauri::command]
pub fn list_app_providers(state: State<'_, IntegrationsState>) -> Vec<AppProviderDto> {
    state.catalog.list()
}

#[tauri::command]
pub fn list_app_action_descriptors(
    state: State<'_, IntegrationsState>,
    provider_id: Option<String>,
) -> Vec<ActionDescriptor> {
    state.action_descriptors(provider_id.as_deref())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_app_action_resources(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    connection_id: String,
    provider_id: String,
    action_id: String,
    field_key: String,
    query: String,
    page_token: Option<String>,
) -> Result<ActionResourcePage, ActionError> {
    state
        .list_action_resources(
            db.inner(),
            &connection_id,
            &provider_id,
            &action_id,
            &field_key,
            &query,
            page_token.as_deref(),
        )
        .await
}

#[tauri::command]
pub fn list_app_connections(
    db: State<'_, Db>,
) -> Result<Vec<AppConnectionDto>, IntegrationCommandError> {
    db.list_app_connections()
        .map(|connections| {
            connections
                .into_iter()
                .map(AppConnectionDto::from)
                .collect()
        })
        .map_err(|_| metadata_read_error())
}

#[tauri::command]
pub fn get_app_connection(
    db: State<'_, Db>,
    id: String,
) -> Result<Option<AppConnectionDto>, IntegrationCommandError> {
    db.get_app_connection(&id)
        .map(|connection| connection.map(AppConnectionDto::from))
        .map_err(|_| metadata_read_error())
}

#[tauri::command]
pub fn get_app_connection_usage(
    db: State<'_, Db>,
    id: String,
) -> Result<AppConnectionUsage, IntegrationCommandError> {
    if db
        .get_app_connection(&id)
        .map_err(|_| metadata_read_error())?
        .is_none()
    {
        return Err(IntegrationCommandError::not_found());
    }
    db.get_app_connection_usage(&id)
        .map_err(|_| metadata_read_error())
}

#[tauri::command]
pub async fn disconnect_app_connection(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    id: String,
    metadata_only: bool,
) -> Result<(), IntegrationCommandError> {
    state.disconnect(db.inner(), &id, metadata_only).await
}

fn metadata_read_error() -> IntegrationCommandError {
    IntegrationCommandError::new(
        "connection_store_failed",
        "Connected-app metadata could not be read.",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::actions::{
        ActionFieldDescriptor, ActionFieldKind, ActionOption, ActionResourceItem,
    };
    use crate::integrations::models::{
        canonical_identity_key, AppConnection, ConnectionStatus, UpsertAppConnection,
    };

    #[test]
    fn every_command_response_type_is_redacted() {
        let provider = AppProviderDto {
            id: "provider".into(),
            name: "Provider".into(),
            capability_summary: "Read records".into(),
            connection_modes: vec!["native_oauth".into()],
            connect_available: false,
        };
        let connection = AppConnectionDto::from(AppConnection {
            id: "id".into(),
            provider_id: "slack".into(),
            display_name: Some("Account".into()),
            external_account_id: None,
            external_tenant_id: None,
            connection_mode: "native_oauth".into(),
            identity_key: "identity-secret-fixture".into(),
            scopes: vec!["read".into()],
            status: ConnectionStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "credential-secret-fixture".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        let usage = AppConnectionUsage::default();
        let descriptor = ActionDescriptor {
            provider_id: "slack".into(),
            action_id: "slack.send_message".into(),
            label: "Send message".into(),
            description: "Send a message".into(),
            fields: vec![ActionFieldDescriptor {
                key: "message".into(),
                label: "Message".into(),
                description: String::new(),
                kind: ActionFieldKind::Textarea,
                required: true,
                default: None,
                secret: false,
                option_source: None,
                options: Vec::<ActionOption>::new(),
                supports_interpolation: true,
            }],
            required_scopes: vec!["chat:write".into()],
            output_schema_version: 1,
        };
        let resources = ActionResourcePage {
            items: vec![ActionResourceItem {
                id: "C123".into(),
                label: "Engineering".into(),
            }],
            next_page_token: None,
        };
        let error = metadata_read_error();
        let serialized = [
            serde_json::to_string(&provider).expect("provider"),
            serde_json::to_string(&connection).expect("connection"),
            serde_json::to_string(&usage).expect("usage"),
            serde_json::to_string(&descriptor).expect("descriptor"),
            serde_json::to_string(&resources).expect("resources"),
            serde_json::to_string(&error).expect("error"),
        ]
        .join(" ");
        assert!(!serialized.contains("identity-secret-fixture"));
        assert!(!serialized.contains("credential-secret-fixture"));
        assert!(!serialized.contains("credentialRef"));
        assert!(!serialized.contains("accessToken"));
    }

    #[test]
    fn repository_fixture_cannot_leak_through_the_dto() {
        let db = Db::open_in_memory().expect("database");
        let saved = db
            .upsert_app_connection(UpsertAppConnection {
                provider_id: "slack".into(),
                display_name: None,
                external_account_id: None,
                external_tenant_id: None,
                connection_mode: "native_oauth".into(),
                identity_key: canonical_identity_key("slack", "native_oauth", &["account"]),
                scopes: vec![],
                expires_at: None,
                credential_ref: "credential-secret-fixture".into(),
            })
            .expect("save");
        let json = serde_json::to_string(&AppConnectionDto::from(saved)).expect("serialize");
        assert!(!json.contains("credential-secret-fixture"));
    }
}
