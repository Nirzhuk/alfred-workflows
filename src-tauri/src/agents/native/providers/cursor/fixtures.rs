use super::*;
use crate::agents::native::{
    NativeCancellation, NativeContextBlock, NativeContextRole, NativeCredential, NativeEventLimits,
    NativeEventNormalizer, NativePermissionProfile, NativeSessionMode, NativeToolCapabilitySet,
    ResolvedNativeAccount, NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::{AgentHarness, AgentProvider, OpaqueAgentAccountRef};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_SECRET: &str = "crsr_cursor_fixture_secret";

struct FakeCursorApiKey(String);

impl std::fmt::Debug for FakeCursorApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FakeCursorApiKey([REDACTED])")
    }
}

impl Drop for FakeCursorApiKey {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

struct FakeCursorTransport;

impl FakeCursorTransport {
    fn authorize(account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider != AgentProvider::Cursor {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "Cursor fixture account belongs to another provider",
                false,
            ));
        }
        let api_key = account
            .credential
            .downcast_ref::<FakeCursorApiKey>()
            .ok_or_else(|| {
                NativeRuntimeError::new(
                    NativeErrorCode::AccountUnavailable,
                    "Cursor fixture API key is unavailable",
                    false,
                )
            })?;
        if api_key.0.is_empty() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "Cursor fixture API key is unavailable",
                false,
            ));
        }
        Ok(())
    }
}

fn resolved_account() -> ResolvedNativeAccount {
    ResolvedNativeAccount {
        account_ref: OpaqueAgentAccountRef::parse("account_cursor-fixture").unwrap(),
        provider: AgentProvider::Cursor,
        product: crate::agent_accounts::models::AgentProductId::CursorCloud,
        credential: NativeCredential::new(FakeCursorApiKey(API_SECRET.into())),
    }
}

fn fixture_request() -> NativeTurnRequest {
    let workspace = PathBuf::from("/workspace/alfred");
    NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: AgentHarness::Alfred,
        harness_version: "1.0.0".into(),
        runtime_version: CURSOR_CLOUD_API_VERSION.into(),
        provider: AgentProvider::Cursor,
        account_ref: OpaqueAgentAccountRef::parse("account_cursor-fixture").unwrap(),
        run_id: "run_fixture".into(),
        node_id: "node_fixture".into(),
        model: "composer-2".into(),
        prompt: "Update the README".into(),
        context: vec![NativeContextBlock {
            role: NativeContextRole::User,
            content: "Update the README".into(),
            name: None,
        }],
        working_directory: workspace.clone(),
        allowed_workspace_roots: vec![workspace],
        permission_profile: NativePermissionProfile::default(),
        tool_capabilities: NativeToolCapabilitySet::default(),
        session_mode: NativeSessionMode::Ephemeral,
        session_id: None,
        event_limits: NativeEventLimits::default(),
        timeout_ms: 300_000,
        cancellation: Some(
            NativeCancellation::new("cursor-fixture", Duration::from_secs(30)).unwrap(),
        ),
    }
}

fn binding() -> CursorRepositoryBinding {
    CursorRepositoryBinding::new(
        PathBuf::from("/workspace/alfred"),
        "https://github.com/example/alfred",
        "main",
    )
    .unwrap()
}

#[test]
fn success_fixture_maps_bounded_request_models_usage_and_events() {
    let account = resolved_account();
    FakeCursorTransport::authorize(&account).unwrap();
    assert!(!format!("{account:?}").contains(API_SECRET));

    let request = fixture_request();
    let payload =
        create_agent_payload(&request, &binding(), Path::new("/workspace/alfred")).unwrap();
    assert_eq!(
        payload["repos"][0]["url"],
        "https://github.com/example/alfred"
    );
    assert_eq!(payload["model"]["id"], "composer-2");
    assert_eq!(payload["workOnCurrentBranch"], false);
    let encoded = serde_json::to_string(&payload).unwrap();
    assert!(!encoded.contains("/workspace/alfred"));
    assert!(!encoded.contains("account_cursor"));

    let models = decode_models(
        200,
        br#"{"items":[{"id":"composer-2","displayName":"Composer 2"}]}"#,
    )
    .unwrap();
    assert_eq!(models[0].id, "composer-2");
    let usage = decode_usage(
        200,
        br#"{"totalUsage":{"inputTokens":12,"outputTokens":5,"cacheWriteTokens":0,"cacheReadTokens":0,"totalTokens":17},"runs":[]}"#,
    )
    .unwrap();
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(5));
    assert_eq!(usage.window_resets_at, None);

    let assistant = map_stream_event("assistant", br#"{"text":"README updated"}"#)
        .unwrap()
        .unwrap();
    assert_eq!(assistant.kind, NativeEventKind::AssistantDelta);
    assert_eq!(assistant.text.as_deref(), Some("README updated"));
    let completed = map_stream_event(
        "result",
        br#"{"runId":"run-1","status":"FINISHED","git":{"branches":[{"repoUrl":"github.com/example/alfred","branch":"cursor/readme"}]}}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(completed.kind, NativeEventKind::TurnCompleted);
    assert_eq!(completed.metadata["executionLocation"], "cloud");
    assert_eq!(
        completed.metadata["gitBranches"][0]["branch"],
        "cursor/readme"
    );
}

#[test]
fn http_401_403_and_429_have_stable_states() {
    let unauthorized = map_http_failure(401, Some("invalid_api_key"));
    assert_eq!(unauthorized.code, NativeErrorCode::AccountUnavailable);
    assert!(!unauthorized.retryable);
    let forbidden = map_http_failure(403, Some("repository_forbidden"));
    assert_eq!(forbidden.code, NativeErrorCode::PermissionDenied);
    assert!(!forbidden.retryable);
    let limited = map_http_failure(429, Some("rate_limited"));
    assert_eq!(limited.code, NativeErrorCode::ProviderUnavailable);
    assert!(limited.retryable);
}

#[test]
fn timeout_and_cancellation_are_terminal_and_build_only_documented_endpoint() {
    assert_eq!(
        map_transport_failure(CursorTransportFailure::Timeout).code,
        NativeErrorCode::TimedOut
    );
    assert_eq!(
        map_transport_failure(CursorTransportFailure::Cancelled).code,
        NativeErrorCode::Cancelled
    );
    let cancelled = map_stream_event("result", br#"{"runId":"run-1","status":"CANCELLED"}"#)
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.kind, NativeEventKind::TurnCancelled);
    assert_eq!(
        cancel_endpoint("bc-123", "run-456").unwrap(),
        "https://api.cursor.com/v1/agents/bc-123/runs/run-456/cancel"
    );
}

#[test]
fn workspace_and_repository_mismatch_never_fall_back_to_local_upload() {
    let request = fixture_request();
    let error =
        create_agent_payload(&request, &binding(), Path::new("/workspace/other")).unwrap_err();
    assert_eq!(error.code, NativeErrorCode::WorkspaceDenied);
    assert_eq!(
        CursorRepositoryBinding::new(
            PathBuf::from("/workspace/alfred"),
            "https://gitlab.com/example/alfred",
            "main",
        )
        .unwrap_err()
        .code,
        NativeErrorCode::WorkspaceDenied
    );

    let mut scoped = request;
    scoped.context.insert(
        0,
        NativeContextBlock {
            role: NativeContextRole::Skill,
            content: "local-only instructions".into(),
            name: Some("local".into()),
        },
    );
    assert_eq!(
        create_agent_payload(&scoped, &binding(), Path::new("/workspace/alfred"))
            .unwrap_err()
            .code,
        NativeErrorCode::InvalidRequest
    );

    let mut secret_prompt = fixture_request();
    secret_prompt.prompt = format!("use {API_SECRET}");
    secret_prompt.context[0].content = secret_prompt.prompt.clone();
    assert_eq!(
        create_agent_payload(&secret_prompt, &binding(), Path::new("/workspace/alfred"))
            .unwrap_err()
            .code,
        NativeErrorCode::InvalidRequest
    );
}

#[test]
fn tool_failure_fixture_is_observational_and_does_not_claim_approval_support() {
    let tool = map_stream_event(
        "tool_call",
        br#"{"callId":"call-1","name":"shell","status":"error","result":{"exitCode":1}}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(tool.kind, NativeEventKind::ToolCompleted);
    assert!(tool.tool_output.as_deref().unwrap().contains("exitCode"));

    let failed = map_stream_event(
        "error",
        br#"{"code":"tool_failed","message":"shell returned 1"}"#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(failed.kind, NativeEventKind::TurnFailed);
    assert_eq!(failed.error.as_deref(), Some("Cursor cloud tool failed"));
    let decision = native_decision();
    assert!(decision.blocked_gates[2].contains("no per-tool approval callback"));
    assert!(!CURSOR_NATIVE_READY);
}

#[test]
fn oversized_output_is_rejected_by_provider_and_shared_event_bounds() {
    let provider_oversized = json!({ "text": "x".repeat(128 * 1024) });
    assert_eq!(
        map_stream_event(
            "assistant",
            &serde_json::to_vec(&provider_oversized).unwrap()
        )
        .unwrap_err()
        .code,
        NativeErrorCode::EventLimitExceeded
    );

    let event = map_stream_event("assistant", br#"{"text":"123456789"}"#)
        .unwrap()
        .unwrap();
    let mut limits = NativeEventLimits::default();
    limits.max_text_bytes = 8;
    let mut normalizer = NativeEventNormalizer::new(limits).unwrap();
    assert_eq!(
        normalizer
            .normalize({
                let mut event = event;
                event.sequence = 1;
                event
            })
            .unwrap_err()
            .code,
        NativeErrorCode::EventLimitExceeded
    );
}

#[test]
fn revoked_key_and_provider_payloads_are_redacted() {
    let body =
        format!(r#"{{"code":"api_key_revoked","message":"Authorization: Bearer {API_SECRET}"}}"#);
    let revoked = map_http_failure(401, decode_error_code(body.as_bytes()).as_deref());
    assert_eq!(revoked.code, NativeErrorCode::AccountUnavailable);
    assert!(revoked.message.contains("revoked"));
    assert!(!revoked.message.contains(API_SECRET));

    let event = map_stream_event(
        "assistant",
        format!(r#"{{"text":"standalone key {API_SECRET}"}}"#).as_bytes(),
    )
    .unwrap()
    .unwrap();
    let mut normalizer = NativeEventNormalizer::new(NativeEventLimits::default()).unwrap();
    let normalized = normalizer
        .normalize({
            let mut event = event;
            event.sequence = 1;
            event
        })
        .unwrap();
    assert!(!normalized.text.unwrap().contains(API_SECRET));
    assert_eq!(
        redact_cursor_text(&format!("before {API_SECRET} after")),
        "before [REDACTED] after"
    );

    let mismatched = ResolvedNativeAccount {
        account_ref: OpaqueAgentAccountRef::parse("account_other-fixture").unwrap(),
        provider: AgentProvider::Codex,
        product: crate::agent_accounts::models::AgentProductId::OpenaiApi,
        credential: NativeCredential::new(FakeCursorApiKey(API_SECRET.into())),
    };
    assert_eq!(
        FakeCursorTransport::authorize(&mismatched)
            .unwrap_err()
            .code,
        NativeErrorCode::AccountMismatch
    );
    assert!(!format!("{mismatched:?}").contains(API_SECRET));
}

#[test]
fn official_surface_is_frozen_but_native_registration_remains_blocked() {
    let decision = native_decision();
    assert_eq!(decision.provider_id, "cursor");
    assert!(decision.selected_surface.contains("Cloud Agents API v1"));
    assert!(decision.auth.contains("API key"));
    assert!(decision.billing_owner.contains("owns the API key"));
    assert!(decision.execution_location.contains("cloud"));
    assert!(decision
        .repository_requirement
        .contains("explicitly confirmed"));
    assert!(decision.models.contains("/v1/models"));
    assert!(decision.usage.contains("token counts"));
    assert!(decision.packaging.contains("no Cursor CLI"));
    assert_eq!(decision.gate_code, CURSOR_NATIVE_GATE_CODE);
    assert_eq!(decision.blocked_gates.len(), 3);
}
