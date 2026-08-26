use crate::agent_accounts::authorization::AuthorizationStartedDto;
use crate::agent_accounts::models::{
    AgentAccountCommandError, AgentAccountDto, AgentApiKeySecret,
    AgentProviderRegistrationDto,
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
        if provider.provider_id == AgentProvider::ClaudeCode.as_str()
            && capability.auth_methods.as_slice() == ["api_key"]
        {
            provider.provider_name = "Claude".into();
        }
    }
    if capability.is_some_and(|entry| {
        api_key_intake_is_approved(&provider.provider_id, entry, available)
    }) {
        provider.connect_available = true;
        provider.gate_code = capability.and_then(|entry| entry.block_reason.clone());
        return;
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
pub async fn connect_agent_api_key_account(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
    manifest: State<'_, crate::agents::capability_manifest::AgentCapabilityManifest>,
    provider_id: String,
    harness: AgentHarness,
    account_id: Option<String>,
    api_key: AgentApiKeySecret,
) -> Result<AgentAccountDto, AgentAccountCommandError> {
    let provider = parse_api_key_provider(&provider_id)?;
    require_api_key_intake_capability(manifest.inner(), provider, harness)?;
    state
        .connect_api_key_account(
            db.inner(),
            provider,
            account_id.as_deref(),
            api_key.into_zeroizing(),
        )
        .await
}

fn parse_api_key_provider(provider_id: &str) -> Result<AgentProvider, AgentAccountCommandError> {
    match provider_id {
        "claude_code" => Ok(AgentProvider::ClaudeCode),
        "gemini" => Ok(AgentProvider::Gemini),
        "grok" => Ok(AgentProvider::Grok),
        _ => Err(AgentAccountCommandError::new(
            "api_key_provider_not_supported",
            "API-key account intake is not available for that native provider.",
            false,
        )),
    }
}

fn api_key_live_smoke_code(provider: AgentProvider) -> Option<&'static str> {
    match provider {
        AgentProvider::ClaudeCode => Some("claude_live_api_key_smoke_missing"),
        AgentProvider::Gemini => Some("gemini_live_api_key_smoke_missing"),
        AgentProvider::Grok => Some("grok_live_api_key_smoke_missing"),
        _ => None,
    }
}

fn api_key_intake_is_approved(
    provider_id: &str,
    capability: &crate::agents::capability_manifest::AgentCapabilityEntry,
    execution_available: bool,
) -> bool {
    let Some(provider) = AgentProvider::from_str(provider_id) else {
        return false;
    };
    let Some(live_smoke_code) = api_key_live_smoke_code(provider) else {
        return false;
    };
    capability.harness == AgentHarness::Alfred
        && capability.auth_methods.as_slice() == ["api_key"]
        && capability.credential_custody == "alfred_managed"
        && (execution_available || capability.block_reason.as_deref() == Some(live_smoke_code))
}

fn require_api_key_intake_capability(
    manifest: &crate::agents::capability_manifest::AgentCapabilityManifest,
    provider: AgentProvider,
    harness: AgentHarness,
) -> Result<(), AgentAccountCommandError> {
    if harness != AgentHarness::Alfred {
        return Err(AgentAccountCommandError::new(
            "native_account_requires_alfred_harness",
            "Native API-key accounts are only available for the Alfred harness.",
            false,
        ));
    }
    if !manifest.is_valid() {
        return Err(AgentAccountCommandError::new(
            "native_capability_manifest_invalid",
            "Native API-key account intake is blocked because the release manifest is invalid.",
            false,
        ));
    }
    let capability = manifest.entry(provider, harness).ok_or_else(|| {
        AgentAccountCommandError::new(
            "native_capability_manifest_entry_missing",
            "Native API-key account intake is not declared by this build.",
            false,
        )
    })?;
    if api_key_intake_is_approved(
        provider.as_str(),
        capability,
        capability.permits_execution(manifest.platform, manifest.build_kind),
    ) {
        return Ok(());
    }
    Err(AgentAccountCommandError::new(
        capability
            .block_reason
            .as_deref()
            .unwrap_or("native_provider_not_available"),
        "Native API-key account intake is blocked by this build's release manifest.",
        false,
    ))
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

        let api_key_dto = AgentAccountDto::from(AgentAccount {
            id: "account_api_key".into(),
            provider: AgentProvider::ClaudeCode,
            harness: AgentHarness::Alfred,
            identity_key: "hashed-identity".into(),
            display_name: Some("API key redacted-label".into()),
            external_account_id: Some("secret-derived-fingerprint".into()),
            external_workspace_id: Some("must-not-cross".into()),
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            scopes: vec![],
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: "agent-account:secret-ref".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        let json = serde_json::to_string(&api_key_dto).expect("serialize API-key DTO");
        assert_eq!(api_key_dto.provider_name, "Claude");
        for hidden in [
            "secret-derived-fingerprint",
            "must-not-cross",
            "agent-account:secret-ref",
        ] {
            assert!(!json.contains(hidden));
        }
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
        assert_eq!(error.code, "gemini_live_api_key_smoke_missing");

        for provider in [
            AgentProvider::ClaudeCode,
            AgentProvider::Gemini,
            AgentProvider::Grok,
        ] {
            assert_eq!(
                parse_api_key_provider(provider.as_str()).unwrap(),
                provider
            );
            assert!(require_api_key_intake_capability(
                &manifest,
                provider,
                AgentHarness::Alfred,
            )
            .is_ok());
        }
        assert_eq!(
            parse_api_key_provider("codex").unwrap_err().code,
            "api_key_provider_not_supported"
        );
        assert_eq!(
            parse_api_key_provider("claude").unwrap_err().code,
            "api_key_provider_not_supported"
        );
        assert_eq!(
            require_api_key_intake_capability(
                &manifest,
                AgentProvider::Gemini,
                AgentHarness::Cli,
            )
            .unwrap_err()
            .code,
            "native_account_requires_alfred_harness"
        );

        let state = AgentAccountsState::default();
        for (provider_id, name, billing, gate) in [
            (
                "claude_code",
                "Claude",
                "anthropic_api_usage_based",
                "claude_live_api_key_smoke_missing",
            ),
            (
                "gemini",
                "Gemini",
                "google_ai_api_usage_based",
                "gemini_live_api_key_smoke_missing",
            ),
            (
                "grok",
                "Grok",
                "xai_api_usage_based",
                "grok_live_api_key_smoke_missing",
            ),
        ] {
            let mut provider = state
                .list_providers()
                .unwrap()
                .into_iter()
                .find(|provider| provider.provider_id == provider_id)
                .unwrap();
            apply_manifest_registration(&mut provider, &manifest);
            assert!(provider.connect_available);
            assert_eq!(provider.provider_name, name);
            assert_eq!(provider.auth_methods, vec!["api_key"]);
            assert_eq!(provider.credential_custody, "alfred_managed");
            assert_eq!(provider.billing_source, billing);
            assert_eq!(provider.gate_code.as_deref(), Some(gate));
        }

        let mut invalid_manifest = manifest.clone();
        invalid_manifest.entries.push(invalid_manifest.entries[0].clone());
        let error = require_account_capability(
            &invalid_manifest,
            AgentProvider::Codex,
            AgentHarness::Alfred,
        )
        .unwrap_err();
        assert_eq!(error.code, "native_capability_manifest_invalid");
        assert_eq!(
            require_api_key_intake_capability(
                &invalid_manifest,
                AgentProvider::Gemini,
                AgentHarness::Alfred,
            )
            .unwrap_err()
            .code,
            "native_capability_manifest_invalid"
        );
    }
}
