//! [`NativeAgentRuntime`] for the native GitHub Copilot harness.

use super::auth::CopilotAccessToken;
use super::entitlement::{classify_rejection, CopilotAccountState};
use super::events::{contains_provider_secret, scrub, CopilotEventMapper, MappedEvent};
use super::transport::{
    CopilotPermissionReply, CopilotSession, CopilotSessionPolicy, CopilotStartError,
    CopilotTransport, UnlinkedSdkTransport,
};
use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agents::native::{
    is_secret_key, AlfredToolKind, AlfredToolRequest, AlfredToolStatus, NativeAgentRuntime,
    NativeCapabilities, NativeErrorCode, NativeEvent, NativeEventKind, NativeModel,
    NativeRuntimeDescriptor, NativeRuntimeError, NativeToolExecutionOwner, NativeTurnHost,
    NativeTurnOutcome, NativeTurnRequest, NativeUsageSnapshot, ResolvedNativeAccount,
    NATIVE_EVENT_CONTRACT_VERSION, NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::AgentProvider;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

pub const RUNTIME_ID: &str = "github-copilot-native";

/// A Copilot turn may not produce more raw SDK frames than this. Frames that
/// map to nothing (heartbeats, reasoning) still count, so a runtime that spins
/// without progressing is cut off rather than looping forever.
const MAX_SDK_FRAMES: usize = 8_192;
const MAX_TOOL_REQUEST_BYTES: usize = 64 * 1024;

pub struct GithubCopilotNativeRuntime {
    transport: Arc<dyn CopilotTransport>,
}

impl GithubCopilotNativeRuntime {
    pub fn new(transport: Arc<dyn CopilotTransport>) -> Self {
        Self { transport }
    }

    /// The runtime Alfred ships with today: fails closed until the SDK crate is
    /// linked into `Cargo.toml`.
    pub fn unlinked() -> Self {
        Self::new(Arc::new(UnlinkedSdkTransport))
    }

    fn token(account: &ResolvedNativeAccount) -> Result<CopilotAccessToken, NativeRuntimeError> {
        let credential = account
            .credential
            .downcast_ref::<NativeAgentCredential>()
            .ok_or_else(|| {
                NativeRuntimeError::new(
                    NativeErrorCode::AccountMismatch,
                    "copilot native runtime received a credential it does not own",
                    false,
                )
            })?;
        let raw = credential.access_token().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "the connected Copilot account has no stored access token",
                false,
            )
        })?;
        CopilotAccessToken::parse(raw).map_err(|code| {
            NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                format!("stored Copilot credential is unusable: {code}"),
                false,
            )
        })
    }
}

impl NativeAgentRuntime for GithubCopilotNativeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: RUNTIME_ID.into(),
            runtime_version: self.transport.runtime_source().version().to_string(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: AgentProvider::GithubCopilot,
            product: crate::agent_accounts::models::AgentProductId::GithubCopilotSubscription,
            tool_execution_owner: NativeToolExecutionOwner::AlfredExecuted,
            capabilities: NativeCapabilities {
                // Alfred's own device flow, not an SDK login.
                supports_oauth: true,
                // BYOK is a documented SDK mode but is not wired here, so it is
                // declared false rather than optimistically true.
                supports_api_key: false,
                supports_sessions: true,
                supports_resume: false,
                supports_model_list: true,
                // GitHub exposes no documented per-seat usage read for an
                // individual account; `usage_snapshot` returns `unavailable`.
                supports_usage: false,
                supports_tool_calls: true,
                supports_approval_events: true,
                // Filesystem and shell work happens on the Alfred tool
                // boundary, never inside the Copilot CLI process.
                supports_native_filesystem: false,
                supports_native_shell: false,
                supports_patch: false,
                supports_mcp: false,
                supports_subagents: false,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider != AgentProvider::GithubCopilot {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "account provider is not github_copilot",
                false,
            ));
        }
        // A parseable token proves GitHub authentication only. Copilot
        // entitlement is proven when a session actually starts, so this stops
        // here on purpose.
        Self::token(account).map(|_| ())
    }

    fn discover_models(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        self.validate_account(account)?;
        Ok(self
            .transport
            .list_models()?
            .into_iter()
            .map(|(id, label)| NativeModel { id, label })
            .collect())
    }

    fn run_turn(
        &self,
        account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        self.validate_account(account)?;
        host.cancellation().checkpoint()?;

        let source = self.transport.runtime_source();
        if !source.is_alfred_managed() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "native Copilot requires an Alfred-managed runtime; a user-installed CLI is not accepted",
                false,
            ));
        }

        let token = Self::token(account)?;
        let policy = CopilotSessionPolicy::alfred_boundary();
        let mut session = match self.transport.start_session(request, &token, &policy) {
            Ok(session) => session,
            Err(CopilotStartError::Runtime(error)) => return Err(error),
            Err(CopilotStartError::Rejected(rejection)) => {
                let login = account.account_ref.as_str();
                let state = classify_rejection(login, &rejection);
                return Err(account_error(&state));
            }
        };
        drop(token);

        let mut mapper = CopilotEventMapper::new();
        let mut frames = 0usize;

        loop {
            if let Err(error) = host.cancellation().checkpoint() {
                // Best-effort abort: the turn is already over either way, so a
                // failure to reach the child process must not mask the reason.
                let _ = session.abort();
                return Err(error);
            }

            frames += 1;
            if frames > MAX_SDK_FRAMES {
                let _ = session.abort();
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "copilot runtime exceeded the supported frame count for one turn",
                    false,
                ));
            }

            let Some(event) = session.next_event()? else {
                break;
            };

            match event.event_type.as_str() {
                "tool.invocation_requested" => {
                    run_alfred_tool(session.as_mut(), host, &event.data)?;
                    continue;
                }
                "permission.requested" => {
                    // Alfred's approval handler is the only approval authority.
                    let request_id = required_permission_request_id(&event.data)?;
                    session.respond_permission(request_id, CopilotPermissionReply::Deny)?;
                    let mut warning = NativeEvent::new(0, NativeEventKind::Warning);
                    warning.text = Some(
                        "Copilot requested a permission outside the Alfred tool boundary; denied."
                            .into(),
                    );
                    host.emit(warning)?;
                    continue;
                }
                _ => {}
            }

            match mapper.map(&event)? {
                MappedEvent::Emit(native) => {
                    let terminal = matches!(
                        native.kind,
                        NativeEventKind::TurnCompleted
                            | NativeEventKind::TurnFailed
                            | NativeEventKind::TurnCancelled
                    );
                    let kind = native.kind;
                    host.emit(native)?;
                    if terminal {
                        if kind == NativeEventKind::TurnCancelled {
                            return Err(NativeRuntimeError::cancelled());
                        }
                        break;
                    }
                }
                MappedEvent::Drop => {}
            }
        }

        Ok(NativeTurnOutcome {
            session_id: mapper.session_id().map(str::to_string),
        })
    }

    fn cancel(
        &self,
        cancellation: &crate::agents::native::NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        // The cooperative flag is what the run loop checks; the live session's
        // `abort()` is issued from inside `run_turn` on the next checkpoint.
        cancellation.cancel();
        Ok(())
    }

    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        // GitHub documents org-level Copilot billing, not a per-seat usage read
        // for an individual user, so this stays honestly unavailable.
        Ok(NativeUsageSnapshot::unavailable())
    }
}

/// Routes one Copilot tool invocation through the Alfred tool boundary and
/// returns its result to Copilot.
fn run_alfred_tool(
    session: &mut dyn CopilotSession,
    host: &mut dyn NativeTurnHost,
    data: &serde_json::Map<String, Value>,
) -> Result<(), NativeRuntimeError> {
    validate_tool_payload_size(data)?;
    reject_secret_tool_fields(data)?;
    let invocation_id = data
        .get("invocationId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "copilot tool invocation is missing its identifier",
                false,
            )
        })?
        .to_string();

    let name = data
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.len() <= 128)
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "copilot tool invocation is missing its name",
                false,
            )
        })?;

    let kind = tool_kind(name).ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "copilot requested a tool Alfred does not expose",
            false,
        )
    })?;

    let mut tool_request = AlfredToolRequest::new(invocation_id.clone(), kind, name);
    if let Some(path) = data.get("path").and_then(Value::as_str) {
        tool_request.path = Some(PathBuf::from(path));
    }
    if let Some(input) = data.get("input").and_then(Value::as_object) {
        tool_request.input = input.clone();
    }
    if let Some(arguments) = data.get("arguments").and_then(Value::as_array) {
        tool_request.arguments = arguments
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }

    // `invoke_tool` owns the tool_started / approval / tool_completed events
    // and enforces the workspace and permission policy.
    let result = host.invoke_tool(tool_request)?;
    let allowed = result.status == AlfredToolStatus::Completed;
    session.respond_tool(&invocation_id, &result)?;
    if !allowed {
        session.respond_permission(&invocation_id, CopilotPermissionReply::Deny)?;
    }
    Ok(())
}

/// Rejects every raw SDK field that can flow into an Alfred tool request.
/// Scrubbing is appropriate for display events, but execution must fail closed
/// so a modified secret is never handed to `host.invoke_tool`.
pub(super) fn reject_secret_tool_fields(
    data: &serde_json::Map<String, Value>,
) -> Result<(), NativeRuntimeError> {
    for field in ["invocationId", "name", "path", "cwd"] {
        if let Some(value) = data.get(field) {
            reject_secret_value(field, value)?;
        }
    }
    for field in ["input", "arguments"] {
        if let Some(value) = data.get(field) {
            reject_secret_value(field, value)?;
        }
    }
    Ok(())
}

fn reject_secret_value(field: &str, value: &Value) -> Result<(), NativeRuntimeError> {
    if contains_secret_value(value) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            format!("copilot tool field {field} contains prohibited secret material"),
            false,
        ));
    }
    let encoded = match value {
        Value::String(value) => value.clone(),
        _ => serde_json::to_string(value).map_err(|_| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "copilot tool invocation is not valid JSON",
                false,
            )
        })?,
    };
    if contains_provider_secret(&encoded) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            format!("copilot tool field {field} contains prohibited secret material"),
            false,
        ));
    }
    Ok(())
}

fn contains_secret_value(value: &Value) -> bool {
    match value {
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| is_secret_key(key) || contains_secret_value(value)),
        Value::Array(values) => values.iter().any(contains_secret_value),
        Value::String(value) => contains_provider_secret(value),
        _ => false,
    }
}

pub(super) fn validate_tool_payload_size(
    data: &serde_json::Map<String, Value>,
) -> Result<(), NativeRuntimeError> {
    let size = serde_json::to_vec(data)
        .map_err(|_| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "copilot tool invocation is not valid JSON",
                false,
            )
        })?
        .len();
    if size > MAX_TOOL_REQUEST_BYTES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "copilot tool invocation exceeds the supported size",
            false,
        ));
    }
    Ok(())
}

pub(super) fn required_permission_request_id(
    data: &serde_json::Map<String, Value>,
) -> Result<&str, NativeRuntimeError> {
    let request_id = data
        .get("requestId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|request_id| !request_id.is_empty() && request_id.len() <= 128)
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "copilot permission request has an invalid identifier",
                false,
            )
        })?;
    if contains_provider_secret(request_id) {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "copilot permission request identifier contains prohibited secret material",
            false,
        ));
    }
    Ok(request_id)
}

/// Alfred's exposed tool names. Copilot's own filesystem and shell tools are
/// filtered off at session start, so this list is the whole surface.
fn tool_kind(name: &str) -> Option<AlfredToolKind> {
    match name {
        "alfred_file_read" => Some(AlfredToolKind::FileRead),
        "alfred_file_write" => Some(AlfredToolKind::FileWrite),
        "alfred_file_edit" => Some(AlfredToolKind::FileEdit),
        "alfred_directory_list" => Some(AlfredToolKind::DirectoryList),
        "alfred_shell" => Some(AlfredToolKind::Shell),
        _ => None,
    }
}

/// Turns an account state into the typed native error the UI reads.
fn account_error(state: &CopilotAccountState) -> NativeRuntimeError {
    let retryable = matches!(state, CopilotAccountState::QuotaExhausted { .. });
    NativeRuntimeError::new(
        NativeErrorCode::AccountUnavailable,
        scrub(state.code()),
        retryable,
    )
}
