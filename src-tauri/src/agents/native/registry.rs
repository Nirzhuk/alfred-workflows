use super::redaction::{contains_cli_permission_flag, contains_secret_marker, redact_text};
use super::{
    normalize_tool_result, prepare_native_request, validate_tool_request, AlfredApprovalDecision,
    AlfredApprovalHandler, AlfredApprovalRequest, AlfredToolExecutor, AlfredToolRequest,
    AlfredToolResult, NativeAccountResolver, NativeAgentRuntime, NativeApprovalPolicy,
    NativeCancellation, NativeCapabilities, NativeContextPolicy, NativeErrorCode, NativeEvent,
    NativeEventKind, NativeEventLimits, NativeEventNormalizer, NativeModel,
    NativePermissionProfile, NativeRuntimeDescriptor, NativeRuntimeError, NativeSessionMode,
    NativeToolCapabilitySet, NativeToolExecutionOwner, NativeTurnHost, NativeTurnOutcome,
    NativeTurnRequest, NativeUsageSnapshot, ResolvedNativeAccount, TOOL_CONTRACT_VERSION,
};
use crate::agent_accounts::models::AgentProductId;
use crate::agents::{
    AgentActivity, AgentActivityKind, AgentActivityState, AgentError, AgentExecutionTarget,
    AgentNativeRuntime, AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
    OpaqueAgentAccountRef,
};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReportStatus {
    Supported,
    Unsupported,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReportEntry {
    pub capability: String,
    pub status: CapabilityReportStatus,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCapabilityReport {
    pub provider: AgentProvider,
    pub product: AgentProductId,
    pub runtime_id: String,
    pub runtime_version: String,
    pub tool_execution_owner: NativeToolExecutionOwner,
    pub entries: Vec<CapabilityReportEntry>,
}

impl NativeCapabilityReport {
    pub fn from_descriptor(descriptor: &NativeRuntimeDescriptor) -> Self {
        let capabilities = [
            ("oauth", descriptor.capabilities.supports_oauth),
            ("api_key", descriptor.capabilities.supports_api_key),
            ("sessions", descriptor.capabilities.supports_sessions),
            ("resume", descriptor.capabilities.supports_resume),
            ("model_list", descriptor.capabilities.supports_model_list),
            ("usage", descriptor.capabilities.supports_usage),
            ("tool_calls", descriptor.capabilities.supports_tool_calls),
            (
                "approval_events",
                descriptor.capabilities.supports_approval_events,
            ),
            (
                "native_filesystem",
                descriptor.capabilities.supports_native_filesystem,
            ),
            (
                "native_shell",
                descriptor.capabilities.supports_native_shell,
            ),
            ("patch", descriptor.capabilities.supports_patch),
            ("mcp", descriptor.capabilities.supports_mcp),
            ("subagents", descriptor.capabilities.supports_subagents),
        ];
        Self {
            provider: descriptor.provider,
            product: descriptor.product,
            runtime_id: descriptor.runtime_id.clone(),
            runtime_version: descriptor.runtime_version.clone(),
            tool_execution_owner: descriptor.tool_execution_owner,
            entries: capabilities
                .into_iter()
                .map(|(capability, supported)| CapabilityReportEntry {
                    capability: capability.into(),
                    status: if supported {
                        CapabilityReportStatus::Supported
                    } else {
                        CapabilityReportStatus::Unsupported
                    },
                    evidence: if supported {
                        "declared by the registered native runtime".into()
                    } else {
                        "explicitly absent from the registered native runtime descriptor".into()
                    },
                })
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct NativeRuntimeRegistry {
    runtimes: RwLock<HashMap<String, Arc<dyn NativeAgentRuntime>>>,
    active: Mutex<HashMap<String, NativeCancellation>>,
}

impl NativeRuntimeRegistry {
    /// Safe readiness signal for the release diagnostics. Descriptors and
    /// provider payloads are intentionally not exposed through this method.
    pub fn contains(&self, provider: AgentProvider) -> bool {
        self.runtimes
            .read()
            .is_ok_and(|runtimes| runtimes.contains_key(provider.as_str()))
    }

    pub fn register(&self, runtime: Arc<dyn NativeAgentRuntime>) -> Result<(), NativeRuntimeError> {
        let descriptor = runtime.descriptor();
        validate_descriptor(&descriptor)?;
        let mut runtimes = self.runtimes.write().map_err(|_| registry_unavailable())?;
        if runtimes.contains_key(descriptor.provider.as_str()) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "a native runtime is already registered for this provider",
                false,
            ));
        }
        runtimes.insert(descriptor.provider.as_str().into(), runtime);
        Ok(())
    }

    pub fn unregister(
        &self,
        provider: AgentProvider,
    ) -> Result<Option<Arc<dyn NativeAgentRuntime>>, NativeRuntimeError> {
        let removed = self
            .runtimes
            .write()
            .map_err(|_| registry_unavailable())?
            .remove(provider.as_str());
        Ok(removed)
    }

    pub fn descriptor(
        &self,
        provider: AgentProvider,
    ) -> Result<NativeRuntimeDescriptor, NativeRuntimeError> {
        Ok(self.runtime(provider)?.descriptor())
    }

    pub fn capability_report(
        &self,
        provider: AgentProvider,
    ) -> Result<NativeCapabilityReport, NativeRuntimeError> {
        self.descriptor(provider)
            .map(|descriptor| NativeCapabilityReport::from_descriptor(&descriptor))
    }

    pub fn validate_account(
        &self,
        provider: AgentProvider,
        account_ref: &OpaqueAgentAccountRef,
        resolver: &dyn NativeAccountResolver,
    ) -> Result<(), NativeRuntimeError> {
        let runtime = self.runtime(provider)?;
        let descriptor = runtime.descriptor();
        let account = resolver
            .resolve(account_ref, provider, descriptor.product)
            .map_err(sanitize_runtime_error)?;
        validate_resolved_account(&account, account_ref, provider, descriptor.product)?;
        runtime
            .validate_account(&account)
            .map_err(sanitize_runtime_error)
    }

    pub fn discover_models(
        &self,
        provider: AgentProvider,
        account_ref: &OpaqueAgentAccountRef,
        resolver: &dyn NativeAccountResolver,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        let runtime = self.runtime(provider)?;
        let descriptor = runtime.descriptor();
        if !descriptor.capabilities.supports_model_list {
            return Err(unsupported("model discovery"));
        }
        let account = resolver
            .resolve(account_ref, provider, descriptor.product)
            .map_err(sanitize_runtime_error)?;
        validate_resolved_account(&account, account_ref, provider, descriptor.product)?;
        runtime
            .validate_account(&account)
            .map_err(sanitize_runtime_error)?;
        let models = runtime
            .discover_models(&account)
            .map_err(sanitize_runtime_error)?;
        validate_models(models)
    }

    pub fn usage_snapshot(
        &self,
        provider: AgentProvider,
        account_ref: &OpaqueAgentAccountRef,
        resolver: &dyn NativeAccountResolver,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        let runtime = self.runtime(provider)?;
        let descriptor = runtime.descriptor();
        if !descriptor.capabilities.supports_usage {
            return Ok(NativeUsageSnapshot::unavailable());
        }
        let account = resolver
            .resolve(account_ref, provider, descriptor.product)
            .map_err(sanitize_runtime_error)?;
        validate_resolved_account(&account, account_ref, provider, descriptor.product)?;
        runtime
            .validate_account(&account)
            .map_err(sanitize_runtime_error)?;
        runtime
            .usage_snapshot(&account)
            .map_err(sanitize_runtime_error)
    }

    pub fn execute_turn(
        &self,
        request: &NativeTurnRequest,
        resolver: &dyn NativeAccountResolver,
        tool_executor: &dyn AlfredToolExecutor,
        approval_handler: &dyn AlfredApprovalHandler,
        on_event: &mut dyn FnMut(&NativeEvent),
    ) -> Result<NativeExecutionResult, NativeRuntimeError> {
        validate_request(request)?;
        let runtime = self.runtime(request.provider)?;
        let descriptor = runtime.descriptor();
        validate_request_for_capabilities(request, &descriptor.capabilities)?;
        if request.runtime_version != descriptor.runtime_version {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "native request runtime version does not match the registered runtime",
                false,
            ));
        }
        let account = resolver
            .resolve(&request.account_ref, request.provider, descriptor.product)
            .map_err(sanitize_runtime_error)?;
        validate_resolved_account(
            &account,
            &request.account_ref,
            request.provider,
            descriptor.product,
        )?;
        runtime
            .validate_account(&account)
            .map_err(sanitize_runtime_error)?;
        validate_selected_model(runtime.as_ref(), &account, &request.model, &descriptor)
            .map_err(sanitize_runtime_error)?;
        let cancellation = request.cancellation()?.clone();
        cancellation.checkpoint()?;
        let active_key = active_key(request.provider, cancellation.id());
        self.active
            .lock()
            .map_err(|_| registry_unavailable())?
            .insert(active_key.clone(), cancellation.clone());

        let result = (|| {
            let mut host = RegistryTurnHost::new(
                request,
                descriptor.tool_execution_owner,
                tool_executor,
                approval_handler,
                on_event,
            )?;
            let outcome = runtime.run_turn(&account, request, &mut host)?;
            cancellation.checkpoint()?;
            Ok(NativeExecutionResult {
                output: host.output,
                events: host.events,
                outcome,
            })
        })();
        let _ = self
            .active
            .lock()
            .map(|mut active| active.remove(&active_key));
        result.map_err(sanitize_runtime_error)
    }

    pub fn cancel(
        &self,
        provider: AgentProvider,
        cancellation_id: &str,
    ) -> Result<(), NativeRuntimeError> {
        let runtime = self.runtime(provider)?;
        let cancellation = self
            .active
            .lock()
            .map_err(|_| registry_unavailable())?
            .get(&active_key(provider, cancellation_id))
            .cloned()
            .ok_or_else(|| {
                NativeRuntimeError::new(
                    NativeErrorCode::InvalidRequest,
                    "native cancellation handle is not active",
                    false,
                )
            })?;
        cancellation.cancel();
        runtime
            .cancel(&cancellation)
            .map_err(sanitize_runtime_error)
    }

    /// Watches a run's Stop flag and cancels the native turn even while the
    /// runtime is blocked between events. The returned guard stops the watcher
    /// when the turn finishes, so no thread outlives its turn.
    pub fn watch_control(
        self: &Arc<Self>,
        provider: AgentProvider,
        control: crate::agents::active::RunControl,
        cancellation: NativeCancellation,
    ) -> NativeControlWatch {
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher = Arc::clone(&finished);
        let registry = Arc::clone(self);
        let cancellation_id = cancellation.id().to_owned();
        let thread = std::thread::spawn(move || loop {
            if watcher.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            if control.is_cancelled() {
                match registry.cancel(provider, &cancellation_id) {
                    Ok(()) => return,
                    // The watcher starts immediately before execute_turn
                    // installs the active handle. Retry that narrow race; if
                    // the turn already completed, dropping the guard stops us.
                    Err(error) if error.code == NativeErrorCode::InvalidRequest => {}
                    Err(_) => {
                        cancellation.cancel();
                        return;
                    }
                }
            }
            if cancellation.is_cancelled() || cancellation.is_timed_out() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        });
        NativeControlWatch {
            finished,
            thread: Some(thread),
        }
    }

    fn runtime(
        &self,
        provider: AgentProvider,
    ) -> Result<Arc<dyn NativeAgentRuntime>, NativeRuntimeError> {
        self.runtimes
            .read()
            .map_err(|_| registry_unavailable())?
            .get(provider.as_str())
            .cloned()
            .ok_or_else(|| {
                NativeRuntimeError::new(
                    NativeErrorCode::ProviderUnavailable,
                    "native runtime is not registered for this provider",
                    false,
                )
            })
    }
}

/// Stops a [`NativeRuntimeRegistry::watch_control`] watcher on drop.
pub struct NativeControlWatch {
    finished: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for NativeControlWatch {
    fn drop(&mut self) {
        self.finished
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeExecutionResult {
    pub output: String,
    pub events: Vec<NativeEvent>,
    pub outcome: NativeTurnOutcome,
}

struct RegistryTurnHost<'a> {
    request: &'a NativeTurnRequest,
    tool_execution_owner: NativeToolExecutionOwner,
    tool_executor: &'a dyn AlfredToolExecutor,
    approval_handler: &'a dyn AlfredApprovalHandler,
    on_event: &'a mut dyn FnMut(&NativeEvent),
    normalizer: NativeEventNormalizer,
    next_sequence: u32,
    output: String,
    events: Vec<NativeEvent>,
}

impl<'a> RegistryTurnHost<'a> {
    fn new(
        request: &'a NativeTurnRequest,
        tool_execution_owner: NativeToolExecutionOwner,
        tool_executor: &'a dyn AlfredToolExecutor,
        approval_handler: &'a dyn AlfredApprovalHandler,
        on_event: &'a mut dyn FnMut(&NativeEvent),
    ) -> Result<Self, NativeRuntimeError> {
        Ok(Self {
            request,
            tool_execution_owner,
            tool_executor,
            approval_handler,
            on_event,
            normalizer: NativeEventNormalizer::new(request.event_limits.clone())?,
            next_sequence: 1,
            output: String::new(),
            events: Vec::new(),
        })
    }

    fn emit_harness_event(&mut self, event: NativeEvent) -> Result<(), NativeRuntimeError> {
        self.emit(event)
    }

    /// A denied tool still terminates its own event pair, so no consumer is
    /// left with a `tool_started` that never completes.
    fn complete_denied(
        &mut self,
        request: &AlfredToolRequest,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        let result = AlfredToolResult::denied(&request.request_id);
        let mut completed = NativeEvent::new(0, NativeEventKind::ToolCompleted);
        completed.tool_call_id = Some(request.request_id.clone());
        completed.tool_name = Some(request.name.clone());
        completed.tool_output = Some(result.output.clone());
        self.emit_harness_event(completed)?;
        Ok(result)
    }
}

impl NativeTurnHost for RegistryTurnHost<'_> {
    fn emit(&mut self, mut event: NativeEvent) -> Result<(), NativeRuntimeError> {
        self.cancellation().checkpoint()?;
        event.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let event = self.normalizer.normalize(event)?;
        if let (NativeEventKind::AssistantDelta, Some(text)) = (event.kind, event.text.as_deref()) {
            if self.output.len().saturating_add(text.len())
                > self.request.event_limits.max_text_bytes
            {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "combined assistant output exceeded the native turn limit",
                    false,
                ));
            }
            self.output.push_str(text);
        }
        (self.on_event)(&event);
        self.events.push(event);
        Ok(())
    }

    fn invoke_tool(
        &mut self,
        request: AlfredToolRequest,
    ) -> Result<AlfredToolResult, NativeRuntimeError> {
        self.cancellation().checkpoint()?;
        if self.tool_execution_owner != NativeToolExecutionOwner::AlfredExecuted {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::CapabilityUnsupported,
                "this runtime does not delegate tool execution to Alfred",
                false,
            ));
        }
        let policy = validate_tool_request(
            &request,
            &self.request.working_directory,
            &self.request.allowed_workspace_roots,
            &self.request.permission_profile,
            &self.request.tool_capabilities,
        )?;
        let mut started = NativeEvent::new(0, NativeEventKind::ToolStarted);
        started.tool_call_id = Some(request.request_id.clone());
        started.tool_name = Some(request.name.clone());
        self.emit_harness_event(started)?;

        if policy == NativeApprovalPolicy::Deny {
            return self.complete_denied(&request);
        }
        if policy == NativeApprovalPolicy::Ask {
            let approval = AlfredApprovalRequest {
                contract_version: TOOL_CONTRACT_VERSION,
                approval_id: format!("approval_{}", request.request_id),
                tool_request_id: request.request_id.clone(),
                tool_name: request.name.clone(),
                kind: request.kind,
            };
            let mut requested = NativeEvent::new(0, NativeEventKind::ApprovalRequested);
            requested.approval_id = Some(approval.approval_id.clone());
            self.emit_harness_event(requested)?;
            let decision = self
                .approval_handler
                .decide(&approval, self.cancellation())?;
            self.cancellation().checkpoint()?;
            let mut resolved = NativeEvent::new(0, NativeEventKind::ApprovalResolved);
            resolved.approval_id = Some(approval.approval_id);
            resolved.approved = Some(decision == AlfredApprovalDecision::Allow);
            self.emit_harness_event(resolved)?;
            if decision == AlfredApprovalDecision::Deny {
                return self.complete_denied(&request);
            }
        }
        let result = self.tool_executor.execute(&request, self.cancellation())?;
        self.cancellation().checkpoint()?;
        let result = normalize_tool_result(result, &request)?;
        let mut completed = NativeEvent::new(0, NativeEventKind::ToolCompleted);
        completed.tool_call_id = Some(request.request_id.clone());
        completed.tool_name = Some(request.name);
        completed.tool_output = Some(result.output.clone());
        self.emit_harness_event(completed)?;
        Ok(result)
    }

    fn cancellation(&self) -> &NativeCancellation {
        self.request
            .cancellation
            .as_ref()
            .expect("validated native request always has cancellation")
    }
}

pub struct NativeExecutionRouter {
    registry: Arc<NativeRuntimeRegistry>,
    resolver: Arc<dyn NativeAccountResolver>,
    tool_executor: Arc<dyn AlfredToolExecutor>,
    approval_handler: Arc<dyn AlfredApprovalHandler>,
    permission_profile: NativePermissionProfile,
    tool_capabilities: NativeToolCapabilitySet,
    event_limits: NativeEventLimits,
    context_policy: NativeContextPolicy,
}

impl NativeExecutionRouter {
    pub fn new(
        registry: Arc<NativeRuntimeRegistry>,
        resolver: Arc<dyn NativeAccountResolver>,
        tool_executor: Arc<dyn AlfredToolExecutor>,
        approval_handler: Arc<dyn AlfredApprovalHandler>,
    ) -> Self {
        Self {
            registry,
            resolver,
            tool_executor,
            approval_handler,
            permission_profile: NativePermissionProfile::default(),
            tool_capabilities: NativeToolCapabilitySet::default(),
            event_limits: NativeEventLimits::default(),
            context_policy: NativeContextPolicy::default(),
        }
    }

    pub fn with_policy(
        mut self,
        permission_profile: NativePermissionProfile,
        tool_capabilities: NativeToolCapabilitySet,
    ) -> Self {
        self.permission_profile = permission_profile;
        self.tool_capabilities = tool_capabilities;
        self
    }
}

impl AgentNativeRuntime for NativeExecutionRouter {
    fn run(
        &self,
        target: &AgentExecutionTarget,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let descriptor = self
            .registry
            .descriptor(target.provider)
            .map_err(native_agent_error)?;
        let request = prepare_native_request(
            target,
            &request,
            &descriptor,
            self.permission_profile.clone(),
            self.tool_capabilities.clone(),
            self.event_limits.clone(),
            &self.context_policy,
        )
        .map_err(native_agent_error)?;
        // Stop cancels the live turn: the registry checkpoints the same handle
        // before every event and tool call, so polling here is enough.
        let _watch = match (hooks.control, request.cancellation.as_ref()) {
            (Some(control), Some(cancellation)) => {
                if control.is_cancelled() {
                    self.registry
                        .runtime(target.provider)
                        .and_then(|runtime| runtime.cancel(cancellation))
                        .map_err(native_agent_error)?;
                }
                Some(self.registry.watch_control(
                    target.provider,
                    control.clone(),
                    cancellation.clone(),
                ))
            }
            _ => None,
        };
        let control = hooks.control.cloned();
        let on_activity = hooks.on_activity;
        let mut forward_event = |event: &NativeEvent| {
            if control
                .as_ref()
                .is_some_and(|control| control.is_cancelled())
            {
                if let Some(cancellation) = request.cancellation.as_ref() {
                    cancellation.cancel();
                }
            }
            if let (Some(on_activity), Some(activity)) = (on_activity, native_event_activity(event))
            {
                on_activity(&activity);
            }
        };
        let result = self
            .registry
            .execute_turn(
                &request,
                self.resolver.as_ref(),
                self.tool_executor.as_ref(),
                self.approval_handler.as_ref(),
                &mut forward_event,
            )
            .map_err(native_agent_error)?;
        Ok(AgentResponse {
            output: result.output,
            metadata: json!({
                "nativeContractVersion": request.contract_version,
                "runtimeId": descriptor.runtime_id,
                "runtimeVersion": descriptor.runtime_version,
                "sessionId": result.outcome.session_id,
            }),
        })
    }
}

/// Maps a normalized native event onto the closed activity vocabulary shared
/// with the CLI harness. `AgentActivity::new` re-sanitizes the label and drops
/// detail, so no provider text can reach a run event through this path.
fn native_event_activity(event: &NativeEvent) -> Option<AgentActivity> {
    let (kind, state, label) = match event.kind {
        NativeEventKind::SessionStarted | NativeEventKind::TurnStarted => (
            AgentActivityKind::Status,
            AgentActivityState::Started,
            "Working",
        ),
        NativeEventKind::AssistantDelta => (
            AgentActivityKind::Assistant,
            AgentActivityState::Started,
            "Responding",
        ),
        NativeEventKind::ToolStarted => (
            AgentActivityKind::Tool,
            AgentActivityState::Started,
            "Using a tool",
        ),
        NativeEventKind::ToolProgress => (
            AgentActivityKind::Tool,
            AgentActivityState::Started,
            "Using a tool",
        ),
        NativeEventKind::ToolCompleted => (
            AgentActivityKind::Tool,
            AgentActivityState::Completed,
            "Used a tool",
        ),
        NativeEventKind::ApprovalRequested => (
            AgentActivityKind::Status,
            AgentActivityState::Started,
            "Waiting for approval",
        ),
        NativeEventKind::ApprovalResolved => (
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Approval resolved",
        ),
        NativeEventKind::Warning => (
            AgentActivityKind::Status,
            AgentActivityState::Started,
            "Working",
        ),
        NativeEventKind::TurnCompleted => (
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Done",
        ),
        NativeEventKind::TurnFailed => (
            AgentActivityKind::Error,
            AgentActivityState::Completed,
            "Failed",
        ),
        NativeEventKind::TurnCancelled => (
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Cancelled",
        ),
    };
    Some(AgentActivity::new(
        format!("native-{}", event.sequence),
        kind,
        state,
        label,
        None,
    ))
}

fn native_agent_error(error: NativeRuntimeError) -> AgentError {
    match error.code {
        NativeErrorCode::Cancelled => AgentError::Cancelled,
        _ => AgentError::Message(
            format!("native_{:?}: {}", error.code, redact_text(&error.message))
                .to_ascii_lowercase()
                .replace(' ', "_"),
        ),
    }
}

fn validate_descriptor(descriptor: &NativeRuntimeDescriptor) -> Result<(), NativeRuntimeError> {
    if descriptor.runtime_id.is_empty()
        || descriptor.runtime_id.len() > 128
        || descriptor.runtime_version.is_empty()
        || descriptor.runtime_version.len() > 64
        || descriptor.request_contract_version != super::NATIVE_REQUEST_CONTRACT_VERSION
        || descriptor.event_contract_version != super::NATIVE_EVENT_CONTRACT_VERSION
        || descriptor.capabilities.contract_version != super::NATIVE_CAPABILITY_CONTRACT_VERSION
        || descriptor.product.provider() != descriptor.provider
        || contains_secret_marker(&descriptor.runtime_id)
        || contains_secret_marker(&descriptor.runtime_version)
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native runtime descriptor is invalid or incompatible",
            false,
        ));
    }
    if descriptor.capabilities.supports_resume && !descriptor.capabilities.supports_sessions {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native runtime cannot support resume without sessions",
            false,
        ));
    }
    if descriptor.capabilities.supports_approval_events
        && !descriptor.capabilities.supports_tool_calls
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native runtime cannot support approvals without tool calls",
            false,
        ));
    }
    match descriptor.tool_execution_owner {
        NativeToolExecutionOwner::NoTools
            if descriptor.capabilities.supports_tool_calls
                || descriptor.capabilities.supports_approval_events =>
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "a no-tools runtime cannot declare tool or approval events",
                false,
            ));
        }
        NativeToolExecutionOwner::AlfredExecuted
            if !descriptor.capabilities.supports_tool_calls =>
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "an Alfred-executed runtime must declare tool calls",
                false,
            ));
        }
        NativeToolExecutionOwner::RuntimeExecutedWithHostApproval
            if !descriptor.capabilities.supports_tool_calls
                || !descriptor.capabilities.supports_approval_events =>
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "a runtime-executed tool route requires host approval events",
                false,
            ));
        }
        _ => {}
    }
    Ok(())
}

fn validate_request(request: &NativeTurnRequest) -> Result<(), NativeRuntimeError> {
    if request.contract_version != super::NATIVE_REQUEST_CONTRACT_VERSION
        || request.harness != crate::agents::AgentHarness::Alfred
        || request.provider.as_str().is_empty()
        || request.harness_version.is_empty()
        || request.harness_version.len() > 64
        || request.model.is_empty()
        || request.model.len() > 256
        || request.run_id.is_empty()
        || request.run_id.len() > 128
        || request.node_id.is_empty()
        || request.node_id.len() > 128
        || request.allowed_workspace_roots.is_empty()
        || request.allowed_workspace_roots.len() > 16
        || request.working_directory.as_os_str().len() > 4_096
        || request
            .allowed_workspace_roots
            .iter()
            .any(|root| root.as_os_str().len() > 4_096)
        || request.timeout_ms == 0
        || request.timeout_ms > super::MAX_TURN_TIMEOUT.as_millis() as u64
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native turn request is invalid or unbounded",
            false,
        ));
    }
    request.event_limits.validate()?;
    request.cancellation()?.checkpoint()?;
    NativeContextPolicy::default().validate_blocks(&request.context)?;
    if request.context.last().map(|block| block.content.as_str()) != Some(request.prompt.as_str()) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native prompt and final user context block do not match",
            false,
        ));
    }
    if let Some(session_id) = request.session_id.as_deref() {
        if session_id.is_empty()
            || session_id.len() > 128
            || !session_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::SessionUnavailable,
                "native session id is invalid or unbounded",
                false,
            ));
        }
    }
    for block in &request.context {
        if contains_secret_marker(&block.content) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "secret-looking credentials are prohibited in native context",
                false,
            ));
        }
        if contains_cli_permission_flag(&block.content) {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::PermissionDenied,
                "CLI permission flags cannot be inherited by native mode",
                false,
            ));
        }
    }
    if request.prompt.is_empty()
        || request.prompt.len() > super::context::DEFAULT_MAX_CONTEXT_BLOCK_BYTES
        || contains_secret_marker(&request.prompt)
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native prompt is empty, unbounded, or secret-bearing",
            false,
        ));
    }
    super::validate_workspace_path(
        &request.working_directory,
        &request.working_directory,
        &request.allowed_workspace_roots,
    )?;
    Ok(())
}

pub(super) fn validate_request_for_capabilities(
    request: &NativeTurnRequest,
    capabilities: &NativeCapabilities,
) -> Result<(), NativeRuntimeError> {
    match request.session_mode {
        NativeSessionMode::Ephemeral => {
            if request.session_id.is_some() {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::SessionUnavailable,
                    "ephemeral native turns cannot carry a session id",
                    false,
                ));
            }
        }
        NativeSessionMode::Start if !capabilities.supports_sessions => {
            return Err(unsupported_session())
        }
        NativeSessionMode::Resume | NativeSessionMode::Fork
            if !capabilities.supports_sessions || !capabilities.supports_resume =>
        {
            return Err(unsupported_session())
        }
        NativeSessionMode::Resume | NativeSessionMode::Fork if request.session_id.is_none() => {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::SessionUnavailable,
                "resume and fork require a bounded native session id",
                false,
            ))
        }
        _ => {}
    }
    let tools_requested = request.tool_capabilities.filesystem
        || request.tool_capabilities.shell
        || request.tool_capabilities.patch
        || request.tool_capabilities.mcp
        || request.tool_capabilities.subagents;
    if tools_requested && !capabilities.supports_tool_calls {
        return Err(unsupported("tool calls"));
    }
    if request.tool_capabilities.filesystem && !capabilities.supports_native_filesystem {
        return Err(unsupported("native filesystem tools"));
    }
    if request.tool_capabilities.shell && !capabilities.supports_native_shell {
        return Err(unsupported("native shell tools"));
    }
    if request.tool_capabilities.patch && !capabilities.supports_patch {
        return Err(unsupported("patch application"));
    }
    if request.tool_capabilities.mcp && !capabilities.supports_mcp {
        return Err(unsupported("MCP tools"));
    }
    if request.tool_capabilities.subagents && !capabilities.supports_subagents {
        return Err(unsupported("subagents"));
    }
    Ok(())
}

fn validate_resolved_account(
    account: &ResolvedNativeAccount,
    expected_ref: &OpaqueAgentAccountRef,
    expected_provider: AgentProvider,
    expected_product: AgentProductId,
) -> Result<(), NativeRuntimeError> {
    if account.provider != expected_provider
        || account.product != expected_product
        || account.account_ref != *expected_ref
    {
        Err(NativeRuntimeError::new(
            NativeErrorCode::AccountMismatch,
            "resolved native account does not match the request",
            false,
        ))
    } else {
        Ok(())
    }
}

fn validate_selected_model(
    runtime: &dyn NativeAgentRuntime,
    account: &ResolvedNativeAccount,
    selected: &str,
    descriptor: &NativeRuntimeDescriptor,
) -> Result<(), NativeRuntimeError> {
    // A provider without a model catalog still runs: the explicitly selected
    // model is bounded and screened instead of matched against a fake list.
    if !descriptor.capabilities.supports_model_list {
        return validate_explicit_model(selected);
    }
    let models = validate_models(runtime.discover_models(account)?)?;
    if models.iter().any(|model| model.id == selected) {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "selected model is not available for this native provider account",
            false,
        ))
    }
}

/// Bounds an explicitly selected model for a runtime that declares no catalog.
fn validate_explicit_model(selected: &str) -> Result<(), NativeRuntimeError> {
    let valid = !selected.is_empty()
        && selected.len() <= 256
        && selected.trim() == selected
        && selected.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
        && !contains_secret_marker(selected);
    if valid {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "explicitly selected native model is invalid or unbounded",
            false,
        ))
    }
}

fn validate_models(models: Vec<NativeModel>) -> Result<Vec<NativeModel>, NativeRuntimeError> {
    if models.is_empty() || models.len() > 512 {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "native model catalog is empty or unbounded",
            false,
        ));
    }
    if models.iter().any(|model| {
        model.id.is_empty()
            || model.id.len() > 256
            || model.label.is_empty()
            || model.label.len() > 256
            || contains_secret_marker(&model.id)
            || contains_secret_marker(&model.label)
    }) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "native model catalog contains an invalid model",
            false,
        ));
    }
    Ok(models)
}

fn unsupported(capability: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::CapabilityUnsupported,
        format!("native runtime does not support {capability}"),
        false,
    )
}

fn unsupported_session() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::SessionUnavailable,
        "native runtime does not support the requested session operation",
        false,
    )
}

fn registry_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        "native runtime registry is unavailable",
        true,
    )
}

fn active_key(provider: AgentProvider, cancellation_id: &str) -> String {
    format!("{}\u{1f}{cancellation_id}", provider.as_str())
}

fn sanitize_runtime_error(mut error: NativeRuntimeError) -> NativeRuntimeError {
    error.message = redact_text(&error.message);
    if error.message.len() > super::DEFAULT_MAX_ERROR_BYTES {
        let mut end = super::DEFAULT_MAX_ERROR_BYTES;
        while !error.message.is_char_boundary(end) {
            end -= 1;
        }
        error.message.truncate(end);
    }
    error
}
