use crate::db::Db;
use crate::integrations::actions::{ActionDescriptor, ActionError, ActionResourcePage};
use crate::integrations::events::{AppEventDescriptor, AppEventError, AppEventResourcePage};
use crate::integrations::github::{GitHubDeviceAuthorization, GitHubDevicePollResult};
use crate::integrations::models::{
    AppConnectionDto, AppConnectionUsage, AppProviderDto, IntegrationCommandError,
};
use crate::integrations::notion::NotionPrivateConnectionInput;
use crate::integrations::obsidian::ObsidianVaultConnectionInput;
use crate::integrations::slack::SlackPrivateConnectionInput;
use crate::integrations::telegram::{
    TelegramCompleteInput, TelegramPairingPrepared, TelegramPrepareInput,
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
pub fn list_app_event_descriptors(
    state: State<'_, IntegrationsState>,
    provider_id: Option<String>,
) -> Vec<AppEventDescriptor> {
    state.events.descriptors(provider_id.as_deref())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_app_event_resources(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    connection_id: String,
    provider_id: String,
    event_type: String,
    field_key: String,
    query: String,
    page_token: Option<String>,
) -> Result<AppEventResourcePage, AppEventError> {
    state
        .list_app_event_resources(
            db.inner(),
            &connection_id,
            &provider_id,
            &event_type,
            &field_key,
            &query,
            page_token.as_deref(),
        )
        .await
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

#[tauri::command]
pub async fn connect_slack_private(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    input: SlackPrivateConnectionInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    state.connect_slack_private(db.inner(), input).await
}

#[tauri::command]
pub async fn prepare_github_connection(
    state: State<'_, IntegrationsState>,
) -> Result<GitHubDeviceAuthorization, IntegrationCommandError> {
    state.prepare_github_connection().await
}

#[tauri::command]
pub async fn poll_github_connection(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    pairing_session_id: String,
) -> Result<GitHubDevicePollResult, IntegrationCommandError> {
    state
        .poll_github_connection(db.inner(), &pairing_session_id)
        .await
}

#[tauri::command]
pub fn cancel_github_pairing(state: State<'_, IntegrationsState>, pairing_session_id: String) {
    state.cancel_github_pairing(&pairing_session_id);
}

#[tauri::command]
pub async fn connect_notion_private(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    input: NotionPrivateConnectionInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    state.connect_notion_private(db.inner(), input).await
}

#[tauri::command]
pub async fn connect_obsidian_vault(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    input: ObsidianVaultConnectionInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    state.connect_obsidian_vault(db.inner(), input).await
}

#[tauri::command]
pub async fn prepare_telegram_connection(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    input: TelegramPrepareInput,
) -> Result<TelegramPairingPrepared, IntegrationCommandError> {
    state.prepare_telegram_connection(db.inner(), input).await
}

#[tauri::command]
pub async fn complete_telegram_connection(
    db: State<'_, Db>,
    state: State<'_, IntegrationsState>,
    input: TelegramCompleteInput,
) -> Result<AppConnectionDto, IntegrationCommandError> {
    state.complete_telegram_connection(db.inner(), input).await
}

#[tauri::command]
pub fn cancel_telegram_pairing(state: State<'_, IntegrationsState>, pairing_session_id: String) {
    state.cancel_telegram_pairing(&pairing_session_id);
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
            experimental: false,
            single_connection: false,
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
            provider_metadata: std::collections::BTreeMap::new(),
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
            output_is_untrusted: false,
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
                provider_metadata: std::collections::BTreeMap::new(),
                expires_at: None,
                credential_ref: "credential-secret-fixture".into(),
            })
            .expect("save");
        let json = serde_json::to_string(&AppConnectionDto::from(saved)).expect("serialize");
        assert!(!json.contains("credential-secret-fixture"));
    }
}
