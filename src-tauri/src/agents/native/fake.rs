use super::*;
use crate::agent_accounts::models::AgentProductId;
use crate::agents::{AgentProvider, OpaqueAgentAccountRef};
use serde_json::Map;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

pub(crate) struct FakeAccountResolver {
    pub account_ref: OpaqueAgentAccountRef,
    pub provider: AgentProvider,
    pub available: AtomicBool,
}

impl FakeAccountResolver {
    pub fn new(account_ref: OpaqueAgentAccountRef, provider: AgentProvider) -> Self {
        Self {
            account_ref,
            provider,
            available: AtomicBool::new(true),
        }
    }
}

impl NativeAccountResolver for FakeAccountResolver {
    fn resolve(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        provider: AgentProvider,
        product: AgentProductId,
    ) -> Result<ResolvedNativeAccount, NativeRuntimeError> {
        if !self.available.load(Ordering::SeqCst) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "fake account is disconnected",
                false,
            ));
        }
        if account_ref != &self.account_ref
            || provider != self.provider
            || product != fake_product(provider)
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "fake account does not match",
                false,
            ));
        }
        Ok(ResolvedNativeAccount {
            account_ref: account_ref.clone(),
            provider,
            product: fake_product(provider),
            credential: NativeCredential::new("fake-credential-held-in-memory".to_string()),
        })
    }
}

fn fake_product(provider: AgentProvider) -> AgentProductId {
    match provider {
        AgentProvider::ClaudeCode => AgentProductId::ClaudeApi,
        AgentProvider::Cursor => AgentProductId::CursorCloud,
        AgentProvider::Codex => AgentProductId::OpenaiApi,
        AgentProvider::Opencode => AgentProductId::OpencodeZen,
        AgentProvider::GithubCopilot => AgentProductId::GithubCopilotSubscription,
        AgentProvider::Gemini => AgentProductId::GeminiApi,
        AgentProvider::Grok => AgentProductId::GrokApi,
        AgentProvider::Pi | AgentProvider::Omp => {
            panic!("no native product is registered for a CLI-only provider")
        }
    }
}

pub(crate) struct FakeNativeRuntime {
    pub provider: AgentProvider,
    pub request_tool: Mutex<Option<AlfredToolRequest>>,
    pub fail_with: Mutex<Option<NativeRuntimeError>>,
    pub cancelled: AtomicBool,
    pub delay_ms: AtomicU64,
}

impl FakeNativeRuntime {
    pub fn new(provider: AgentProvider) -> Self {
        Self {
            provider,
            request_tool: Mutex::new(None),
            fail_with: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            delay_ms: AtomicU64::new(0),
        }
    }

    fn capabilities(&self) -> NativeCapabilities {
        NativeCapabilities {
            supports_api_key: true,
            supports_model_list: true,
            supports_usage: true,
            supports_tool_calls: true,
            supports_approval_events: true,
            supports_native_filesystem: true,
            supports_native_shell: true,
            supports_patch: true,
            ..NativeCapabilities::default()
        }
    }
}

impl NativeAgentRuntime for FakeNativeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: "fake-native-runtime".into(),
            runtime_version: "1.0.0".into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: self.provider,
            product: fake_product(self.provider),
            tool_execution_owner: NativeToolExecutionOwner::AlfredExecuted,
            capabilities: self.capabilities(),
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider != self.provider
            || account
                .credential
                .downcast_ref::<String>()
                .map(String::as_str)
                != Some("fake-credential-held-in-memory")
        {
            Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "fake runtime rejected account",
                false,
            ))
        } else {
            Ok(())
        }
    }

    fn discover_models(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        Ok(vec![NativeModel {
            id: "fake-model".into(),
            label: "Fake Model".into(),
        }])
    }

    fn run_turn(
        &self,
        _account: &ResolvedNativeAccount,
        _request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        host.cancellation().checkpoint()?;
        let delay_ms = self.delay_ms.load(Ordering::SeqCst);
        for _ in 0..delay_ms {
            std::thread::sleep(std::time::Duration::from_millis(1));
            host.cancellation().checkpoint()?;
        }
        if let Some(error) = self.fail_with.lock().expect("fake failure lock").clone() {
            return Err(error);
        }
        host.emit(NativeEvent::new(0, NativeEventKind::TurnStarted))?;
        if let Some(tool) = self.request_tool.lock().expect("fake tool lock").clone() {
            let result = host.invoke_tool(tool)?;
            let text = match result.status {
                AlfredToolStatus::Denied => "tool denied".to_string(),
                _ => format!("tool: {}", result.output),
            };
            let mut delta = NativeEvent::new(0, NativeEventKind::AssistantDelta);
            delta.content_class = Some(NativeContentClass::Assistant);
            delta.text = Some(text);
            host.emit(delta)?;
        } else {
            let mut delta = NativeEvent::new(0, NativeEventKind::AssistantDelta);
            delta.content_class = Some(NativeContentClass::Assistant);
            delta.text = Some("fake response".into());
            host.emit(delta)?;
        }
        host.emit(NativeEvent::new(0, NativeEventKind::TurnCompleted))?;
        Ok(NativeTurnOutcome { session_id: None })
    }

    fn cancel(&self, cancellation: &NativeCancellation) -> Result<(), NativeRuntimeError> {
        self.cancelled.store(true, Ordering::SeqCst);
        cancellation.cancel();
        Ok(())
    }

    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        Ok(NativeUsageSnapshot {
            state: NativeUsageState::Supported,
            input_tokens: Some(7),
            output_tokens: Some(3),
            window_resets_at: None,
        })
    }
}

/// A runtime that declares no model catalog. It still executes turns with an
/// explicitly selected, bounded model.
pub(crate) struct FakeCatalogFreeRuntime {
    pub provider: AgentProvider,
}

impl NativeAgentRuntime for FakeCatalogFreeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: "fake-catalog-free-runtime".into(),
            runtime_version: "1.0.0".into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: self.provider,
            product: fake_product(self.provider),
            tool_execution_owner: NativeToolExecutionOwner::NoTools,
            capabilities: NativeCapabilities {
                supports_api_key: true,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(&self, _account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        Ok(())
    }

    fn discover_models(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        Err(NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "this runtime publishes no model catalog",
            false,
        ))
    }

    fn run_turn(
        &self,
        _account: &ResolvedNativeAccount,
        _request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        let mut delta = NativeEvent::new(0, NativeEventKind::AssistantDelta);
        delta.content_class = Some(NativeContentClass::Assistant);
        delta.text = Some("catalog-free response".into());
        host.emit(delta)?;
        Ok(NativeTurnOutcome { session_id: None })
    }
}

pub(crate) struct FakeToolExecutor {
    pub output: String,
}

impl AlfredToolExecutor for FakeToolExecutor {
    fn execute(
        &self,
        request: &AlfredToolRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        cancellation.checkpoint()?;
        Ok(AlfredToolResult {
            contract_version: TOOL_CONTRACT_VERSION,
            request_id: request.request_id.clone(),
            status: AlfredToolStatus::Completed,
            output: self.output.clone(),
            exit_code: Some(0),
            truncated: false,
            metadata: Map::new(),
        })
    }
}

pub(crate) struct FakeApprovalHandler(pub AlfredApprovalDecision);

impl AlfredApprovalHandler for FakeApprovalHandler {
    fn decide(
        &self,
        _request: &AlfredApprovalRequest,
        cancellation: &NativeCancellation,
    ) -> Result<AlfredApprovalDecision, NativeRuntimeError> {
        cancellation.checkpoint()?;
        Ok(self.0)
    }
}

pub(crate) fn fake_request(
    provider: AgentProvider,
    account_ref: OpaqueAgentAccountRef,
) -> NativeTurnRequest {
    let workspace = std::env::current_dir().expect("current directory");
    NativeTurnRequest {
        contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
        harness: crate::agents::AgentHarness::Alfred,
        harness_version: env!("CARGO_PKG_VERSION").into(),
        runtime_version: "1.0.0".into(),
        provider,
        account_ref,
        run_id: "run_fake".into(),
        node_id: "node_fake".into(),
        model: "fake-model".into(),
        prompt: "hello".into(),
        context: vec![NativeContextBlock {
            role: NativeContextRole::User,
            content: "hello".into(),
            name: None,
        }],
        working_directory: workspace.clone(),
        allowed_workspace_roots: vec![workspace],
        permission_profile: NativePermissionProfile::default(),
        tool_capabilities: NativeToolCapabilitySet {
            filesystem: true,
            shell: true,
            patch: true,
            mcp: false,
            subagents: false,
        },
        session_mode: NativeSessionMode::Ephemeral,
        session_id: None,
        event_limits: NativeEventLimits::default(),
        timeout_ms: DEFAULT_TURN_TIMEOUT.as_millis() as u64,
        cancellation: Some(
            NativeCancellation::new("cancel_fake", DEFAULT_TURN_TIMEOUT)
                .expect("fake cancellation"),
        ),
    }
}

pub(crate) fn shell_tool(arguments: Vec<String>) -> AlfredToolRequest {
    AlfredToolRequest {
        contract_version: TOOL_CONTRACT_VERSION,
        request_id: "tool_fake".into(),
        kind: AlfredToolKind::Shell,
        name: "shell".into(),
        path: Some(PathBuf::from(".")),
        arguments,
        input: Map::new(),
        timeout_ms: 1_000,
        max_output_bytes: 32,
    }
}
