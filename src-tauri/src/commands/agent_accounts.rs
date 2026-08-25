use crate::agent_accounts::authorization::AuthorizationStartedDto;
use crate::agent_accounts::models::{
    AgentAccountCommandError, AgentAccountDto, AgentProviderRegistrationDto,
};
use crate::agent_accounts::AgentAccountsState;
use crate::agents::{AgentHarness, AgentProvider};
use crate::db::Db;
use tauri::State;

#[tauri::command]
pub fn list_agent_account_providers(
    state: State<'_, AgentAccountsState>,
    manifest: State<'_, crate::agents::capability_manifest::AgentCapabilityManifest>,
) -> Result<Vec<AgentProviderRegistrationDto>, AgentAccountCommandError> {
    let mut providers = state.list_providers()?;
    for provider in &mut providers {
        apply_manifest_registration(provider, manifest.inner());
    }
    Ok(providers)
}

fn apply_manifest_registration(
    provider: &mut AgentProviderRegistrationDto,
    manifest: &crate::agents::capability_manifest::AgentCapabilityManifest,
) {
    if !manifest.is_valid() {
        provider.connect_available = false;
        provider.gate_code = Some("native_capability_manifest_invalid".into());
        return;
    }
    let capability = AgentProvider::from_str(&provider.provider_id)
        .and_then(|provider_id| manifest.entry(provider_id, provider.harness));
    let available = capability.is_some_and(|entry| {
        entry.permits_execution(manifest.platform, manifest.build_kind)
    });
    if let Some(capability) = capability {
        provider.auth_methods = capability.auth_methods.clone();
        provider.billing_source = capability.billing_source.clone();
        provider.credential_custody = capability.credential_custody.clone();
    }
    if !available {
        provider.connect_available = false;
        provider.gate_code = Some(
            capability
                .and_then(|entry| entry.block_reason.clone())
                .unwrap_or_else(|| "native_capability_manifest_entry_missing".into()),
        );
    }
}

#[tauri::command]
pub fn list_agent_accounts(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
) -> Result<Vec<AgentAccountDto>, AgentAccountCommandError> {
    state.list_accounts(db.inner())
}

#[tauri::command]
pub fn get_agent_account(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
    id: String,
) -> Result<Option<AgentAccountDto>, AgentAccountCommandError> {
    state.get_account(db.inner(), &id)
}

#[tauri::command]
pub fn start_agent_authorization(
    state: State<'_, AgentAccountsState>,
    manifest: State<'_, crate::agents::capability_manifest::AgentCapabilityManifest>,
    provider_id: String,
    harness: AgentHarness,
) -> Result<AuthorizationStartedDto, AgentAccountCommandError> {
    let provider = AgentProvider::from_str(&provider_id).ok_or_else(|| {
        AgentAccountCommandError::new(
            "provider_not_found",
            "The native agent provider is unknown.",
            false,
        )
    })?;
    require_account_capability(manifest.inner(), provider, harness)?;
    state.start_authorization(&provider_id, harness)
}

#[tauri::command]
pub async fn complete_agent_authorization(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
    manifest: State<'_, crate::agents::capability_manifest::AgentCapabilityManifest>,
    attempt_id: String,
    provider_id: String,
    harness: AgentHarness,
    // The `state` value returned by the provider callback. Required whenever
    // the attempt recorded one; the service refuses a missing or wrong value.
    completion_state: Option<String>,
) -> Result<AgentAccountDto, AgentAccountCommandError> {
    let provider = AgentProvider::from_str(&provider_id).ok_or_else(|| {
        AgentAccountCommandError::new(
            "provider_not_found",
            "The native agent provider is unknown.",
            false,
        )
    })?;
    require_account_capability(manifest.inner(), provider, harness)?;
    state
        .complete_authorization(
            db.inner(),
            &attempt_id,
            &provider_id,
            harness,
            completion_state.as_deref(),
        )
        .await
}

#[tauri::command]
pub fn cancel_agent_authorization(
    state: State<'_, AgentAccountsState>,
    attempt_id: String,
) -> Result<(), AgentAccountCommandError> {
    state.cancel_authorization(&attempt_id)
}

#[tauri::command]
pub async fn refresh_agent_account(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
    manifest: State<'_, crate::agents::capability_manifest::AgentCapabilityManifest>,
    id: String,
) -> Result<AgentAccountDto, AgentAccountCommandError> {
    let account = db
        .get_agent_account(&id)
        .map_err(|_| {
            AgentAccountCommandError::new(
                "account_store_failed",
                "Native-agent account details could not be read.",
                true,
            )
        })?
        .ok_or_else(|| {
            AgentAccountCommandError::new(
                "account_not_found",
                "That native-agent account no longer exists.",
                false,
            )
        })?;
    require_account_capability(manifest.inner(), account.provider, account.harness)?;
    state.refresh_account(db.inner(), &id).await
}

fn require_account_capability(
    manifest: &crate::agents::capability_manifest::AgentCapabilityManifest,
    provider: AgentProvider,
    harness: AgentHarness,
) -> Result<(), AgentAccountCommandError> {
    if harness != AgentHarness::Alfred {
        return Err(AgentAccountCommandError::new(
            "native_account_requires_alfred_harness",
            "Native authorization is only available for the Alfred harness.",
            false,
        ));
    }
    if !manifest.is_valid() {
        return Err(AgentAccountCommandError::new(
            "native_capability_manifest_invalid",
            "Native authorization is blocked because the release manifest is invalid.",
            false,
        ));
    }
    let capability = manifest.entry(provider, harness);
    if capability.is_some_and(|entry| {
        entry.permits_execution(manifest.platform, manifest.build_kind)
    }) {
        return Ok(());
    }
    let code = capability
        .and_then(|entry| entry.block_reason.as_deref())
        .unwrap_or("native_capability_manifest_entry_missing");
    Err(AgentAccountCommandError::new(
        code,
        "Native authorization is blocked by this build's release manifest.",
        false,
    ))
}

#[tauri::command]
pub async fn disconnect_agent_account(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
    id: String,
    metadata_only: bool,
) -> Result<(), AgentAccountCommandError> {
    state.disconnect_account(db.inner(), &id, metadata_only).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::models::{
        AgentAccount, AgentAccountStatus, AgentAuthMethod, CredentialCustodyMode,
    };
    use crate::agents::AgentProvider;

    #[test]
    fn command_dto_has_no_credential_or_identity_reference() {
        let dto = AgentAccountDto::from(AgentAccount {
            id: "account_opaque".into(),
            provider: AgentProvider::Codex,
            harness: AgentHarness::Alfred,
            identity_key: "identity-secret".into(),
            display_name: Some("Account".into()),
            external_account_id: Some("external".into()),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::OAuthPkce,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            scopes: vec![],
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "credential-secret".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        let json = serde_json::to_string(&dto).expect("serialize");
        assert!(!json.contains("credential-secret"));
        assert!(!json.contains("identity-secret"));
        assert!(!json.contains("accessToken"));
        assert!(!json.contains("refreshToken"));
    }

    #[test]
    fn manifest_blocks_account_lifecycle_before_handlers_and_drives_presentation() {
        let manifest = crate::agents::capability_manifest::manifest();
        for provider in [
            AgentProvider::ClaudeCode,
            AgentProvider::Cursor,
            AgentProvider::Codex,
            AgentProvider::Opencode,
            AgentProvider::GithubCopilot,
            AgentProvider::Gemini,
            AgentProvider::Grok,
            AgentProvider::Pi,
            AgentProvider::Omp,
        ] {
            assert!(require_account_capability(&manifest, provider, AgentHarness::Alfred).is_err());
            assert!(require_account_capability(&manifest, provider, AgentHarness::Cli).is_err());
        }
        let error = require_account_capability(
            &manifest,
            AgentProvider::Gemini,
            AgentHarness::Alfred,
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            "gemini_api_key_account_intake_and_live_smoke_missing"
        );

        let state = AgentAccountsState::default();
        let mut provider = state
            .list_providers()
            .unwrap()
            .into_iter()
            .find(|provider| provider.provider_id == "gemini")
            .unwrap();
        apply_manifest_registration(&mut provider, &manifest);
        assert!(!provider.connect_available);
        assert_eq!(provider.auth_methods, vec!["api_key"]);
        assert_eq!(provider.credential_custody, "alfred_managed");
        assert_eq!(provider.billing_source, "google_ai_api_usage_based");
        assert_eq!(
            provider.gate_code.as_deref(),
            Some("gemini_api_key_account_intake_and_live_smoke_missing")
        );

        let mut invalid_manifest = manifest.clone();
        invalid_manifest.entries.push(invalid_manifest.entries[0].clone());
        let error = require_account_capability(
            &invalid_manifest,
            AgentProvider::Codex,
            AgentHarness::Alfred,
        )
        .unwrap_err();
        assert_eq!(error.code, "native_capability_manifest_invalid");
    }
}
