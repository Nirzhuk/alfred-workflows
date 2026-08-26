//! Non-shipping managed-runtime adapter for the stable public Codex SDK.
//!
//! The adapter is complete enough for sealed-package integration and fake
//! conformance tests. Production registration remains fail-closed until every
//! release gate below is cleared in shared release engineering.

use super::{
    validate_codex_sdk_selection, CodexSdkInbound, CodexSdkLogout, CodexSdkMethod,
    CodexSdkProtocol, CodexSdkProtocolError, CODEX_SDK_HOST_APPROVAL_BLOCKER,
    CODEX_SDK_RUNTIME_VERSION,
};
use crate::agent_accounts::models::{AgentProductId, CredentialCustodyMode, ManagedRuntimeId};
use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agent_accounts::runtime_profile::{
    RuntimeEnvironmentVariable, RuntimeProfile, RuntimeProfileBinding, RuntimeProfileLifecycle,
    RuntimeProfileRef, RuntimeProfileStore,
};
use crate::agents::managed_runtime::{
    ManagedRuntimeCancellation, ManagedRuntimeHandle, ManagedRuntimeLaunchSpec,
    ManagedRuntimeLifecycle, ManagedRuntimeSupervisor, RuntimeReadinessProbe, RuntimeShutdownHook,
    RuntimeStdoutPolicy,
};
use crate::agents::native::{
    CapabilityReportStatus, NativeErrorCode, NativeRuntimeError, NativeRuntimeRegistry,
    ResolvedNativeAccount,
};
use crate::agents::runtime_package::RuntimePackageSelection;
use crate::agents::AgentProvider;
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PUBLIC_CAPABILITY_AUDIT_BLOCKER: &str =
    "codex_python_sdk_public_capability_audit_blocked";
pub const KNOWN_CLIENT_ENTERPRISE_BLOCKER: &str =
    "codex_python_sdk_known_client_enterprise_clearance_missing";
pub const PACKAGED_SMOKE_BLOCKER: &str = "codex_python_sdk_packaged_smoke_missing";

const READY_LINE: &str =
    r#"{"experimentalApi":false,"protocolVersion":1,"sdkVersion":"0.147.0","type":"ready"}"#;
const SHUTDOWN_LINE: &str =
    r#"{"method":"shutdown","params":{},"protocolVersion":1,"requestId":"supervisor_shutdown"}"#;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSdkSurfaceStatus {
    Supported,
    Blocked,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSdkSurfaceEntry {
    pub capability: &'static str,
    pub status: CodexSdkSurfaceStatus,
    pub evidence: &'static str,
}

pub fn codex_sdk_surface() -> Vec<CodexSdkSurfaceEntry> {
    vec![
        CodexSdkSurfaceEntry {
            capability: "browser_login",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public Codex.login_chatgpt handle with wait and cancel",
        },
        CodexSdkSurfaceEntry {
            capability: "device_code_login",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public Codex.login_chatgpt_device_code handle with wait and cancel",
        },
        CodexSdkSurfaceEntry {
            capability: "account_logout_models",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public Codex.account, Codex.logout, and Codex.models",
        },
        CodexSdkSurfaceEntry {
            capability: "threads_and_streamed_turns",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public thread_start, thread_resume, turn, and TurnHandle.stream",
        },
        CodexSdkSurfaceEntry {
            capability: "turn_cancellation",
            status: CodexSdkSurfaceStatus::Supported,
            evidence: "public TurnHandle.interrupt",
        },
        CodexSdkSurfaceEntry {
            capability: "host_approvals",
            status: CodexSdkSurfaceStatus::Blocked,
            evidence: CODEX_SDK_HOST_APPROVAL_BLOCKER,
        },
        CodexSdkSurfaceEntry {
            capability: "usage",
            status: CodexSdkSurfaceStatus::Unavailable,
            evidence: "not projected by the audited public stable SDK slice",
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSdkReleaseGate {
    pub gate: &'static str,
    pub status: CapabilityReportStatus,
    pub evidence: &'static str,
}

pub fn codex_sdk_release_gates() -> Vec<CodexSdkReleaseGate> {
    vec![
        CodexSdkReleaseGate {
            gate: "public_capability_audit",
            status: CapabilityReportStatus::Blocked,
            evidence: PUBLIC_CAPABILITY_AUDIT_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "host_approval",
            status: CapabilityReportStatus::Blocked,
            evidence: CODEX_SDK_HOST_APPROVAL_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "known_client_enterprise",
            status: CapabilityReportStatus::Blocked,
            evidence: KNOWN_CLIENT_ENTERPRISE_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "sealed_package",
            status: CapabilityReportStatus::Blocked,
            evidence: super::SEALED_PACKAGE_BLOCKER,
        },
        CodexSdkReleaseGate {
            gate: "packaged_no_dependency_smoke",
            status: CapabilityReportStatus::Blocked,
            evidence: PACKAGED_SMOKE_BLOCKER,
        },
    ]
}

pub fn codex_sdk_native_ready() -> bool {
    codex_sdk_release_gates()
        .iter()
        .all(|gate| gate.status != CapabilityReportStatus::Blocked)
}

/// No shipping runtime is registered while the stable public SDK lacks host
/// approval control and the enterprise/package/smoke evidence is incomplete.
pub fn register(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!(
            "{PUBLIC_CAPABILITY_AUDIT_BLOCKER}; {CODEX_SDK_HOST_APPROVAL_BLOCKER}; \
             {KNOWN_CLIENT_ENTERPRISE_BLOCKER}; {}; {PACKAGED_SMOKE_BLOCKER}",
            super::SEALED_PACKAGE_BLOCKER,
        ),
        false,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSdkRuntimeErrorCode {
    AccountMismatch,
    CredentialRejected,
    ProfileMismatch,
    PackageRejected,
    InvalidWorkingDirectory,
    LaunchRejected,
    ProtocolRejected,
    RuntimeCrashed,
    TimedOut,
    LogoutRejected,
    ProfilePurgeRejected,
}

impl CodexSdkRuntimeErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccountMismatch => "codex_python_sdk_account_mismatch",
            Self::CredentialRejected => "codex_python_sdk_credential_rejected",
            Self::ProfileMismatch => "codex_python_sdk_profile_mismatch",
            Self::PackageRejected => "codex_python_sdk_package_rejected",
            Self::InvalidWorkingDirectory => "codex_python_sdk_cwd_invalid",
            Self::LaunchRejected => "codex_python_sdk_launch_rejected",
            Self::ProtocolRejected => "codex_python_sdk_protocol_rejected",
            Self::RuntimeCrashed => "codex_python_sdk_runtime_crashed",
            Self::TimedOut => "codex_python_sdk_timed_out",
            Self::LogoutRejected => "codex_python_sdk_logout_rejected",
            Self::ProfilePurgeRejected => "codex_python_sdk_profile_purge_rejected",
        }
    }
}

pub struct CodexSdkRuntimeError(CodexSdkRuntimeErrorCode);

impl CodexSdkRuntimeError {
    pub fn code(&self) -> CodexSdkRuntimeErrorCode {
        self.0
    }
}

impl fmt::Debug for CodexSdkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl fmt::Display for CodexSdkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl std::error::Error for CodexSdkRuntimeError {}

pub type RuntimeResult<T> = Result<T, CodexSdkRuntimeError>;

fn runtime_error(code: CodexSdkRuntimeErrorCode) -> CodexSdkRuntimeError {
    CodexSdkRuntimeError(code)
}

pub fn validate_codex_sdk_account(
    account: &ResolvedNativeAccount,
    profile: &RuntimeProfile,
) -> RuntimeResult<()> {
    if account.provider != AgentProvider::Codex || account.product != AgentProductId::ChatgptCodex {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::AccountMismatch));
    }
    let credential = account
        .credential
        .downcast_ref::<NativeAgentCredential>()
        .ok_or_else(|| runtime_error(CodexSdkRuntimeErrorCode::CredentialRejected))?;
    if credential.custody_mode() != CredentialCustodyMode::RuntimeManaged
        || credential.managed_runtime_id() != Some(ManagedRuntimeId::CodexPythonSdk)
        || credential.managed_runtime_version() != Some(CODEX_SDK_RUNTIME_VERSION)
        || credential.runtime_profile_ref() != Some(profile.profile_ref().as_str())
        || credential.access_token().is_some()
        || credential.refresh_token().is_some()
        || credential.runtime_credential_ref().is_some()
        || credential.provider_field("api_key").is_some()
        || credential.provider_field("access_token").is_some()
        || credential.provider_field("refresh_token").is_some()
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::CredentialRejected));
    }
    validate_profile_binding(profile, &account.account_ref)
}

fn validate_profile_binding(
    profile: &RuntimeProfile,
    account_ref: &crate::agents::OpaqueAgentAccountRef,
) -> RuntimeResult<()> {
    let expected = RuntimeProfileBinding::new(
        account_ref,
        AgentProductId::ChatgptCodex,
        ManagedRuntimeId::CodexPythonSdk,
        CODEX_SDK_RUNTIME_VERSION,
    )
    .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProfileMismatch))?;
    let binding = profile.binding();
    let has_codex_home = profile
        .environment_roots()
        .get(RuntimeEnvironmentVariable::CodexHome)
        .is_some();
    if profile.lifecycle() != RuntimeProfileLifecycle::Active
        || binding != &expected
        || !has_codex_home
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::ProfileMismatch));
    }
    Ok(())
}

/// Candidate launch boundary used by provider-local integration tests. It is
/// crate-private so callers cannot bypass the blocked production `register()`.
pub(crate) fn launch_codex_sdk_candidate(
    supervisor: &ManagedRuntimeSupervisor,
    account: &ResolvedNativeAccount,
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
    working_directory: &Path,
    cancellation: ManagedRuntimeCancellation,
) -> RuntimeResult<CodexSdkConnection> {
    validate_codex_sdk_account(account, profile)?;
    validate_codex_sdk_selection(package)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::PackageRejected))?;
    if package.expectation().runtime_id() != profile.binding().runtime_id()
        || package.expectation().runtime_version() != profile.binding().runtime_version()
    {
        return Err(runtime_error(CodexSdkRuntimeErrorCode::PackageRejected));
    }
    let working_directory = canonical_working_directory(working_directory)?;
    let readiness = RuntimeReadinessProbe::stdout_line_equals(READY_LINE)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LaunchRejected))?;
    let shutdown = RuntimeShutdownHook::stdin_line(SHUTDOWN_LINE)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LaunchRejected))?;
    let spec = ManagedRuntimeLaunchSpec::new(
        Vec::new(),
        readiness,
        shutdown,
        RuntimeStdoutPolicy::TypedFramesFailClosed,
    )
    .with_working_directory(working_directory)
    .with_startup_timeout(STARTUP_TIMEOUT)
    .with_shutdown_timeout(SHUTDOWN_TIMEOUT);
    let handle = supervisor
        .launch(package, profile, spec, cancellation)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LaunchRejected))?;
    Ok(CodexSdkConnection {
        handle,
        protocol: Mutex::new(CodexSdkProtocol::default()),
    })
}

fn canonical_working_directory(path: &Path) -> RuntimeResult<PathBuf> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::InvalidWorkingDirectory))?;
    if !path.is_absolute() || !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(runtime_error(
            CodexSdkRuntimeErrorCode::InvalidWorkingDirectory,
        ));
    }
    path.canonicalize()
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::InvalidWorkingDirectory))
}

pub(crate) struct CodexSdkConnection {
    handle: ManagedRuntimeHandle,
    protocol: Mutex<CodexSdkProtocol>,
}

impl fmt::Debug for CodexSdkConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexSdkConnection")
            .field("lifecycle", &self.handle.snapshot().lifecycle)
            .finish()
    }
}

impl CodexSdkConnection {
    pub(crate) fn send(
        &self,
        request_id: &str,
        method: CodexSdkMethod,
        params: Value,
    ) -> RuntimeResult<()> {
        let frame = self
            .protocol
            .lock()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
            .encode_request(request_id, method, params)
            .map_err(map_protocol_error)?;
        self.handle
            .write_stdin_frame(&frame)
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))
    }

    pub(crate) fn track_login_operation(&self, login_id: &str) -> RuntimeResult<()> {
        self.protocol
            .lock()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
            .track_login_operation(login_id)
            .map_err(map_protocol_error)
    }

    pub(crate) fn track_turn_operation(
        &self,
        operation_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> RuntimeResult<()> {
        self.protocol
            .lock()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
            .track_turn_operation(operation_id, thread_id, turn_id)
            .map_err(map_protocol_error)
    }

    pub(crate) fn receive(&self, timeout: Duration) -> RuntimeResult<Option<CodexSdkInbound>> {
        let frame = self
            .handle
            .read_stdout_frame(timeout)
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))?;
        match frame {
            Some(frame) => self
                .protocol
                .lock()
                .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected))?
                .ingest(&frame)
                .map(Some)
                .map_err(map_protocol_error),
            None if matches!(
                self.handle.snapshot().lifecycle,
                ManagedRuntimeLifecycle::Exited | ManagedRuntimeLifecycle::Failed
            ) =>
            {
                if let Ok(mut protocol) = self.protocol.lock() {
                    protocol.process_exited();
                }
                Err(runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))
            }
            None => Ok(None),
        }
    }

    /// Logs the SDK out, waits for the token-free acknowledgement, stops the
    /// supervised process, and only then returns a receipt that authorizes a
    /// matching profile purge.
    pub(crate) fn logout_and_stop(
        self,
        request_id: &str,
        profile: &RuntimeProfile,
        timeout: Duration,
    ) -> RuntimeResult<CodexSdkLogoutReceipt> {
        self.send(
            request_id,
            CodexSdkMethod::Logout,
            super::sdk_protocol::empty_params(),
        )?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| runtime_error(CodexSdkRuntimeErrorCode::TimedOut))?;
        let logout = loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(runtime_error(CodexSdkRuntimeErrorCode::TimedOut));
            }
            let remaining = deadline.saturating_duration_since(now);
            match self.receive(remaining)? {
                Some(CodexSdkInbound::Response(response))
                    if response.request_id == request_id
                        && response.method == CodexSdkMethod::Logout.as_str() =>
                {
                    break super::sdk_protocol::parse_result::<CodexSdkLogout>(&response)
                        .map_err(map_protocol_error)?;
                }
                Some(CodexSdkInbound::Error(error))
                    if error.request_id.as_deref() == Some(request_id) =>
                {
                    return Err(runtime_error(CodexSdkRuntimeErrorCode::LogoutRejected));
                }
                Some(_) => {
                    return Err(runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected));
                }
                None => continue,
            }
        };
        logout
            .validate()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::LogoutRejected))?;
        self.handle
            .stop()
            .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::RuntimeCrashed))?;
        Ok(CodexSdkLogoutReceipt {
            profile_ref: profile.profile_ref().clone(),
            binding: profile.binding().clone(),
        })
    }
}

fn map_protocol_error(_error: CodexSdkProtocolError) -> CodexSdkRuntimeError {
    runtime_error(CodexSdkRuntimeErrorCode::ProtocolRejected)
}

pub struct CodexSdkLogoutReceipt {
    profile_ref: RuntimeProfileRef,
    binding: RuntimeProfileBinding,
}

impl fmt::Debug for CodexSdkLogoutReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CodexSdkLogoutReceipt([REDACTED PROFILE])")
    }
}

/// Profile deletion is impossible without a receipt created after the public
/// SDK's logout acknowledgement and supervisor shutdown.
pub fn purge_logged_out_codex_profile(
    store: &RuntimeProfileStore,
    receipt: CodexSdkLogoutReceipt,
) -> RuntimeResult<()> {
    store
        .purge(&receipt.profile_ref, &receipt.binding)
        .map_err(|_| runtime_error(CodexSdkRuntimeErrorCode::ProfilePurgeRejected))
}
