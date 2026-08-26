use crate::agent_accounts::authorization::AuthorizationStartedDto;
use crate::agent_accounts::models::{
    AgentAccountCommandError, AgentAccountDto, AgentApiKeySecret, AgentProductId,
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
    let capability = provider
        .product_id
        .parse::<AgentProductId>()
        .ok()
        .and_then(|product| manifest.product_entry(product, provider.harness));
    let available = capability
        .is_some_and(|entry| entry.permits_execution(manifest.platform, manifest.build_kind));
    if capability
        .is_some_and(|entry| api_key_intake_is_approved(&provider.product_id, entry, available))
    {
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
    product_id: String,
    harness: AgentHarness,
    account_id: Option<String>,
    api_key: AgentApiKeySecret,
) -> Result<AgentAccountDto, AgentAccountCommandError> {
    let (provider, product) = parse_api_key_product(&provider_id, &product_id)?;
    require_api_key_intake_capability(manifest.inner(), product, harness)?;
    state
        .connect_api_key_account(
            db.inner(),
            provider,
            product,
            account_id.as_deref(),
            api_key.into_zeroizing(),
        )
        .await
}

fn parse_api_key_product(
    provider_id: &str,
    product_id: &str,
) -> Result<(AgentProvider, AgentProductId), AgentAccountCommandError> {
    let product = product_id.parse::<AgentProductId>().map_err(|_| {
        AgentAccountCommandError::new(
            "api_key_provider_not_supported",
            "API-key account intake is not available for that native product.",
            false,
        )
    })?;
    if !product.uses_alfred_managed_api_key() || product.api_key_intake_gate_code().is_none() {
        return Err(AgentAccountCommandError::new(
            "api_key_provider_not_supported",
            "API-key account intake is not available for that native product.",
            false,
        ));
    }
    let provider = product.provider();
    if provider.as_str() != provider_id {
        return Err(AgentAccountCommandError::new(
            "provider_mismatch",
            "The API product is not registered for that provider.",
            false,
        ));
    }
    Ok((provider, product))
}

fn api_key_intake_is_approved(
    product_id: &str,
    capability: &crate::agents::capability_manifest::AgentCapabilityEntry,
    execution_available: bool,
) -> bool {
    let Ok(product) = product_id.parse::<AgentProductId>() else {
        return false;
    };
    if !product.uses_alfred_managed_api_key() {
        return false;
    }
    let Some(live_smoke_code) = product.api_key_intake_gate_code() else {
        return false;
    };
    capability.harness == AgentHarness::Alfred
        && capability.product == Some(product)
        && capability.auth_methods.as_slice() == ["api_key"]
        && capability.credential_custody == "alfred_managed"
        && (execution_available || capability.block_reason.as_deref() == Some(live_smoke_code))
}

fn require_api_key_intake_capability(
    manifest: &crate::agents::capability_manifest::AgentCapabilityManifest,
    product: AgentProductId,
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
    let capability = manifest.product_entry(product, harness).ok_or_else(|| {
        AgentAccountCommandError::new(
            "native_capability_manifest_entry_missing",
            "Native API-key account intake is not declared by this build.",
            false,
        )
    })?;
    if api_key_intake_is_approved(
        product.as_str(),
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
    product_id: String,
    harness: AgentHarness,
) -> Result<AuthorizationStartedDto, AgentAccountCommandError> {
    let provider = AgentProvider::from_str(&provider_id).ok_or_else(|| {
        AgentAccountCommandError::new(
            "provider_not_found",
            "The native agent provider is unknown.",
            false,
        )
    })?;
    let product = product_id.parse::<AgentProductId>().map_err(|_| {
        AgentAccountCommandError::new(
            "product_not_found",
            "The native agent product is unknown.",
            false,
        )
    })?;
    if product.provider() != provider {
        return Err(AgentAccountCommandError::new(
            "provider_mismatch",
            "The product is not registered for that provider.",
            false,
        ));
    }
    require_account_capability(manifest.inner(), product, harness)?;
    state.start_authorization(&provider_id, &product_id, harness)
}

#[tauri::command]
pub async fn complete_agent_authorization(
    db: State<'_, Db>,
    state: State<'_, AgentAccountsState>,
    manifest: State<'_, crate::agents::capability_manifest::AgentCapabilityManifest>,
    attempt_id: String,
    provider_id: String,
    product_id: String,
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
    let product = product_id.parse::<AgentProductId>().map_err(|_| {
        AgentAccountCommandError::new(
            "product_not_found",
            "The native agent product is unknown.",
            false,
        )
    })?;
    if product.provider() != provider {
        return Err(AgentAccountCommandError::new(
            "provider_mismatch",
            "The product is not registered for that provider.",
            false,
        ));
    }
    require_account_capability(manifest.inner(), product, harness)?;
    state
        .complete_authorization(
            db.inner(),
            &attempt_id,
            &provider_id,
            &product_id,
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
    require_account_capability(manifest.inner(), account.product, account.harness)?;
    state.refresh_account(db.inner(), &id).await
}

fn require_account_capability(
    manifest: &crate::agents::capability_manifest::AgentCapabilityManifest,
    product: AgentProductId,
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
    let capability = manifest.product_entry(product, harness);
    if capability
        .is_some_and(|entry| entry.permits_execution(manifest.platform, manifest.build_kind))
    {
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
    state
        .disconnect_account(db.inner(), &id, metadata_only)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::models::{
        AgentAccount, AgentAccountStatus, AgentAuthMethod, AgentEntitlementState,
        CredentialCustodyMode,
    };
    use crate::agents::AgentProvider;

    #[test]
    fn command_dto_has_no_credential_or_identity_reference() {
        let dto = AgentAccountDto::from(AgentAccount {
            id: "account_opaque".into(),
            provider: AgentProvider::Codex,
            product: AgentProductId::OpenaiApi,
            harness: AgentHarness::Alfred,
            identity_key: "identity-secret".into(),
            display_name: Some("Account".into()),
            external_account_id: Some("external".into()),
            external_workspace_id: None,
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            managed_runtime_id: None,
            managed_runtime_version: None,
            runtime_profile_ref: None,
            scopes: vec![],
            billing_source: "provider_api".into(),
            billing_owner: "credential_owner".into(),
            entitlement_state: AgentEntitlementState::Unknown,
            entitlement_source: "provider_unobserved".into(),
            entitlement_observed_at: None,
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: Some("credential-secret".into()),
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
            product: AgentProductId::ClaudeApi,
            harness: AgentHarness::Alfred,
            identity_key: "hashed-identity".into(),
            display_name: Some("API key redacted-label".into()),
            external_account_id: Some("secret-derived-fingerprint".into()),
            external_workspace_id: Some("must-not-cross".into()),
            auth_method: AgentAuthMethod::ApiKey,
            custody_mode: CredentialCustodyMode::AlfredManaged,
            managed_runtime_id: None,
            managed_runtime_version: None,
            runtime_profile_ref: None,
            scopes: vec![],
            billing_source: "provider_api".into(),
            billing_owner: "credential_owner".into(),
            entitlement_state: AgentEntitlementState::Unknown,
            entitlement_source: "provider_unobserved".into(),
            entitlement_observed_at: None,
            status: AgentAccountStatus::Connected,
            expires_at: None,
            last_checked_at: None,
            last_error_code: None,
            credential_ref: Some("agent-account:secret-ref".into()),
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
        for product in AgentProductId::ALL {
            assert!(require_account_capability(&manifest, product, AgentHarness::Alfred).is_err());
            assert!(require_account_capability(&manifest, product, AgentHarness::Cli).is_err());
        }
        let error =
            require_account_capability(&manifest, AgentProductId::GeminiApi, AgentHarness::Alfred)
                .unwrap_err();
        assert_eq!(error.code, "gemini_live_api_key_smoke_missing");

        for (provider, product) in [
            (AgentProvider::ClaudeCode, AgentProductId::ClaudeApi),
            (AgentProvider::Gemini, AgentProductId::GeminiApi),
            (AgentProvider::Grok, AgentProductId::GrokApi),
        ] {
            assert_eq!(
                parse_api_key_product(provider.as_str(), product.as_str()).unwrap(),
                (provider, product)
            );
            assert!(
                require_api_key_intake_capability(&manifest, product, AgentHarness::Alfred,)
                    .is_ok()
            );
        }
        assert_eq!(
            parse_api_key_product("codex", "chatgpt_codex")
                .unwrap_err()
                .code,
            "api_key_provider_not_supported"
        );
        assert_eq!(
            parse_api_key_product("claude_code", "gemini_api")
                .unwrap_err()
                .code,
            "provider_mismatch"
        );
        assert_eq!(
            require_api_key_intake_capability(
                &manifest,
                AgentProductId::GeminiApi,
                AgentHarness::Cli,
            )
            .unwrap_err()
            .code,
            "native_account_requires_alfred_harness"
        );

        let state = AgentAccountsState::default();
        for (product_id, name, gate) in [
            (
                "claude_api",
                "Claude API",
                "claude_live_api_key_smoke_missing",
            ),
            (
                "gemini_api",
                "Gemini API",
                "gemini_live_api_key_smoke_missing",
            ),
            ("grok_api", "Grok API", "grok_live_api_key_smoke_missing"),
        ] {
            let mut provider = state
                .list_providers()
                .unwrap()
                .into_iter()
                .find(|provider| provider.product_id == product_id)
                .unwrap();
            apply_manifest_registration(&mut provider, &manifest);
            assert!(provider.connect_available);
            assert_eq!(provider.product_name, name);
            assert_eq!(provider.auth_methods, vec!["api_key"]);
            assert_eq!(provider.credential_custody, "alfred_managed");
            assert_eq!(provider.billing_source, "provider_api");
            assert_eq!(provider.billing_owner, "credential_owner");
            assert_eq!(provider.gate_code.as_deref(), Some(gate));
        }

        for product_id in ["claude_code_subscription", "openai_api", "opencode_zen"] {
            let mut provider = state
                .list_providers()
                .unwrap()
                .into_iter()
                .find(|provider| provider.product_id == product_id)
                .unwrap();
            apply_manifest_registration(&mut provider, &manifest);
            assert!(!provider.connect_available);
            assert_eq!(
                provider.gate_code.as_deref(),
                Some("native_capability_manifest_entry_missing")
            );
        }

        let mut invalid_manifest = manifest.clone();
        invalid_manifest
            .entries
            .push(invalid_manifest.entries[0].clone());
        let error = require_account_capability(
            &invalid_manifest,
            AgentProductId::ChatgptCodex,
            AgentHarness::Alfred,
        )
        .unwrap_err();
        assert_eq!(error.code, "native_capability_manifest_invalid");
        assert_eq!(
            require_api_key_intake_capability(
                &invalid_manifest,
                AgentProductId::GeminiApi,
                AgentHarness::Alfred,
            )
            .unwrap_err()
            .code,
            "native_capability_manifest_invalid"
        );
    }
}
