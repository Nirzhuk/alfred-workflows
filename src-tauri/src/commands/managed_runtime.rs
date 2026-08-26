//! Shared control plane for Alfred-managed provider runtimes.
//!
//! This module owns the safe command/state boundary around the provider
//! slices.  It deliberately does not enable a provider: package installation,
//! profile creation, and connection start all remain unavailable until the
//! provider's release gates and independent package evidence are present.

use crate::agent_accounts::authorization::AuthorizationAttempt;
use crate::agent_accounts::models::{AgentAccount, AgentApiKeySecret, AgentProductId};
use crate::agent_accounts::runtime_profile::RuntimeProfileStore;
use crate::agent_accounts::service::{
    AgentAccountProvider, AgentProviderError, AgentProviderFailureKind, AgentProviderRegistration,
    ProviderAccountAccess, ProviderAccountGrant, ProviderAuthorizationStart, ProviderFuture,
};
use crate::agents::managed_runtime::{ManagedRuntimeCancellation, ManagedRuntimeSupervisor};
use crate::agents::native::providers::claude::{
    ClaudeCodeSubscriptionRuntime, ClaudePublisherVerificationError,
    ClaudePublisherVerificationRequest, ClaudeTerminalError, ClaudeTerminalLaunchSpec,
    ClaudeTerminalSession,
};
use crate::agents::native::providers::codex::{
    CodexSdkPackageError, CodexSdkPackageVerifier, CodexSdkVerifierRequest,
};
use crate::agents::native::providers::opencode::OpenCodePackageVerifier;
use crate::agents::native::providers::opencode::OpenCodeServerSession;
use crate::agents::native::providers::{claude, codex, opencode};
use crate::agents::native::{NativeCancellation, NativeErrorCode, NativeRuntimeError};
use crate::agents::runtime_package::{
    verify_runtime_package_with_platform_evidence, RuntimePackageError, RuntimePackageManifest,
    RuntimePackageSelection, RuntimePackageStore, RuntimePackageVerification,
    RuntimePlatformPublisherVerifier, UnavailableRuntimePlatformPublisherVerifier,
};
use crate::agents::AgentProvider;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

const CLAUDE_PRODUCT: AgentProductId = AgentProductId::ClaudeCodeSubscription;
const CODEX_PRODUCT: AgentProductId = AgentProductId::ChatgptCodex;
const OPENCODE_PRODUCT: AgentProductId = AgentProductId::OpencodeGo;

/// Exact public product projection consumed by the managed-runtime settings
/// surface.  All fields are descriptive and safe; no profile or credential
/// reference crosses this boundary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeProductDto {
    pub provider_id: String,
    pub product_id: String,
    pub product_name: String,
    pub runtime_id: String,
    pub runtime_version: String,
    pub install_state: String,
    pub connection_kind: String,
    pub connect_available: bool,
    pub gate_codes: Vec<String>,
    pub billing_source: String,
    pub custody_mode: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeConnectionStartDto {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    // This is intentionally present even when no provider ceremony is
    // active, so the public contract has an explicit nullable expiry.
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeConnectionStatusDto {
    pub provider_id: String,
    pub product_id: String,
    pub install_state: String,
    pub connection_state: String,
    pub account_id: Option<String>,
    pub entitlement_state: String,
    pub last_error_code: Option<String>,
}

/// Public terminal read projection. Provider session ids are opaque handles;
/// provider-specific output framing remains behind this command boundary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeTerminalReadDto {
    pub session_id: String,
    pub cursor: u64,
    pub output: String,
    pub closed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeCommandError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl ManagedRuntimeCommandError {
    fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
        }
    }
}

impl fmt::Display for ManagedRuntimeCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ManagedRuntimeCommandError {}

#[derive(Debug, Clone)]
struct ManagedProductSpec {
    provider: AgentProvider,
    product: AgentProductId,
    runtime_id: crate::agent_accounts::models::ManagedRuntimeId,
    runtime_version: &'static str,
    connection_kind: &'static str,
    gate_codes: Vec<String>,
    primary_gate_code: &'static str,
}

impl ManagedProductSpec {
    fn dto(&self, backend_ready: bool) -> ManagedRuntimeProductDto {
        let mut gate_codes = self.gate_codes.clone();
        if !backend_ready {
            gate_codes.push("managed_runtime_storage_unavailable".into());
        }
        ManagedRuntimeProductDto {
            provider_id: self.provider.as_str().into(),
            product_id: self.product.as_str().into(),
            product_name: self.product.label().into(),
            runtime_id: self.runtime_id.as_str().into(),
            runtime_version: self.runtime_version.into(),
            // The only path to `active` is a sealed package selection held by
            // this process and an active profile.  No such claim is made by
            // the current blocked release.
            install_state: if backend_ready { "missing" } else { "blocked" }.into(),
            connection_kind: self.connection_kind.into(),
            connect_available: backend_ready && gate_codes.is_empty(),
            gate_codes,
            billing_source: self.product.billing_source().into(),
            custody_mode: self.product.custody_mode().as_str().into(),
        }
    }
}

#[derive(Clone)]
struct ManagedRuntimeBackend {
    packages: RuntimePackageStore,
    profiles: RuntimeProfileStore,
    resource_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ManagedConnection {
    kind: String,
    state: String,
    attempt_id: Option<String>,
    authorization_url: Option<String>,
    user_code: Option<String>,
    terminal_session_id: Option<String>,
}

/// Process-wide state.  The supervisor is long-lived and backend-only; the
/// package/profile stores are initialized from Tauri's app-data directory in
/// setup.  Commands remain safe if setup cannot initialize storage.
pub struct ManagedRuntimeControlPlane {
    supervisor: ManagedRuntimeSupervisor,
    trust_verifier: ManagedRuntimeTrustVerifier,
    backend: Mutex<Option<ManagedRuntimeBackend>>,
    connections: Mutex<HashMap<String, ManagedConnection>>,
    terminal_sessions: Mutex<HashMap<String, Arc<ClaudeTerminalSession>>>,
}

impl ManagedRuntimeControlPlane {
    pub fn new() -> Self {
        Self {
            supervisor: ManagedRuntimeSupervisor::new(),
            trust_verifier: ManagedRuntimeTrustVerifier::unavailable(),
            backend: Mutex::new(None),
            connections: Mutex::new(HashMap::new()),
            terminal_sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Initialize once from Tauri-owned paths.  It never creates a provider
    /// package or profile and therefore cannot make an unavailable product
    /// appear connected.
    pub fn initialize(
        &self,
        app_data_root: &Path,
        resource_root: Option<&Path>,
    ) -> Result<(), ManagedRuntimeCommandError> {
        let packages = RuntimePackageStore::open(app_data_root)
            .map_err(|error| package_command_error(error, true))?;
        let profiles = RuntimeProfileStore::new(app_data_root)
            .map_err(|error| profile_command_error(error.to_string(), true))?;
        let mut backend = self.backend.lock().map_err(|_| state_error())?;
        if backend.is_none() {
            *backend = Some(ManagedRuntimeBackend {
                packages,
                profiles,
                resource_root: resource_root.map(Path::to_path_buf),
            });
        }
        Ok(())
    }

    pub fn supervisor(&self) -> &ManagedRuntimeSupervisor {
        &self.supervisor
    }

    pub fn trust_verifier(&self) -> &ManagedRuntimeTrustVerifier {
        &self.trust_verifier
    }

    /// Returns the only bridge allowed to connect an OpenCode supervisor
    /// launch to its fixed-loopback authenticated client. The bridge owns the
    /// one-way password handoff and never exposes the password to commands.
    pub fn supervisor_http_bridge(&self) -> Arc<dyn opencode::OpenCodeSupervisorHttpBridge> {
        Arc::new(ManagedRuntimeSupervisorHttpBridge)
    }

    pub fn provider_handlers(self: &Arc<Self>) -> Vec<Arc<dyn AgentAccountProvider>> {
        managed_products()
            .into_iter()
            .map(|spec| {
                Arc::new(ManagedRuntimeAccountProvider {
                    control_plane: Arc::clone(self),
                    spec,
                }) as Arc<dyn AgentAccountProvider>
            })
            .collect()
    }

    pub fn list_products(
        &self,
    ) -> Result<Vec<ManagedRuntimeProductDto>, ManagedRuntimeCommandError> {
        let backend_ready = self.backend.lock().map_err(|_| state_error())?.is_some();
        Ok(managed_products()
            .into_iter()
            .map(|spec| spec.dto(backend_ready))
            .collect())
    }

    pub fn prepare_product(
        &self,
        provider_id: &str,
        product_id: &str,
    ) -> Result<ManagedRuntimeProductDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        // The source root is retained only as an internal future package input
        // and is never returned.  This command intentionally does not scan
        // arbitrary filesystem paths or claim an artifact from metadata.
        let backend_ready = self.backend.lock().map_err(|_| state_error())?.is_some();
        Ok(spec.dto(backend_ready))
    }

    pub fn start_connection(
        &self,
        provider_id: &str,
        product_id: &str,
    ) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        let status = self.product_status(&spec)?;
        if !status.connect_available {
            return Err(ManagedRuntimeCommandError::new(
                spec.primary_gate_code,
                "This managed provider product is blocked by its release gates.",
                false,
            ));
        }
        // Keep this branch structurally unreachable until the package/profile
        // and provider auth implementations are released.  In particular,
        // never synthesize a CLI fallback or an unverified connection.
        Err(ManagedRuntimeCommandError::new(
            "managed_runtime_connection_unavailable",
            "The managed runtime connection is not available in this build.",
            false,
        ))
    }

    /// Compatibility command for the OpenCode Go settings surface. The
    /// secret is accepted through the same zeroizing input type as existing
    /// account commands, then dropped while the product's release gates keep
    /// the provider unavailable. It must not be persisted or treated as an
    /// active connection until the provider slice and package are released.
    pub fn connect_api_key(
        &self,
        provider_id: &str,
        product_id: &str,
        _api_key: Zeroizing<String>,
    ) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        if spec.product != OPENCODE_PRODUCT {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_api_key_product_invalid",
                "Managed API-key intake is only available for OpenCode Go.",
                false,
            ));
        }
        Err(ManagedRuntimeCommandError::new(
            spec.primary_gate_code,
            "OpenCode Go managed API-key intake is blocked by its release gates.",
            false,
        ))
    }

    pub fn connection_status(
        &self,
        provider_id: &str,
        product_id: &str,
    ) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
        let spec = product_spec(provider_id, product_id)?;
        let key = spec.product.as_str().to_owned();
        let connection = self
            .connections
            .lock()
            .map_err(|_| state_error())?
            .get(&key)
            .cloned();
        let product_status = self.product_status(&spec)?;
        let install_state = product_status.install_state;
        let last_error_code = product_status.gate_codes.first().cloned();
        let fallback_connection_state = if product_status.connect_available {
            "disconnected"
        } else {
            "error"
        };
        let connection_state = connection
            .as_ref()
            .map(|connection| match connection.state.as_str() {
                "connecting" => "connecting",
                "connected" => "connected",
                "limited" => "limited",
                "disconnected" => "disconnected",
                _ => "error",
            })
            .unwrap_or(fallback_connection_state);
        Ok(ManagedRuntimeConnectionStatusDto {
            provider_id: spec.provider.as_str().into(),
            product_id: spec.product.as_str().into(),
            install_state,
            connection_state: connection_state.into(),
            account_id: None,
            entitlement_state: "unknown".into(),
            last_error_code,
        })
    }

    fn product_status(
        &self,
        spec: &ManagedProductSpec,
    ) -> Result<ManagedRuntimeProductDto, ManagedRuntimeCommandError> {
        let backend_ready = self.backend.lock().map_err(|_| state_error())?.is_some();
        Ok(spec.dto(backend_ready))
    }

    fn terminal(
        &self,
        session_id: &str,
    ) -> Result<Arc<ClaudeTerminalSession>, ManagedRuntimeCommandError> {
        if !valid_terminal_session_id(session_id) {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_terminal_session_invalid",
                "The managed terminal session is invalid.",
                false,
            ));
        }
        self.terminal_sessions
            .lock()
            .map_err(|_| state_error())?
            .get(session_id)
            .cloned()
            .ok_or_else(|| {
                ManagedRuntimeCommandError::new(
                    "managed_runtime_terminal_not_found",
                    "The managed terminal session no longer exists.",
                    false,
                )
            })
    }

    fn read_terminal(
        &self,
        session_id: &str,
        cursor: u64,
    ) -> Result<ManagedRuntimeTerminalReadDto, ManagedRuntimeCommandError> {
        let session = self.terminal(session_id)?;
        // Polling is deliberately non-blocking at the public boundary. The
        // transport timeout is an internal policy, not a command argument.
        let output = session
            .read_output(Duration::ZERO)
            .map_err(terminal_command_error)?;
        let closed = session.snapshot().lifecycle != claude::ClaudeTerminalLifecycle::Running;
        Ok(match output {
            Some(output) => ManagedRuntimeTerminalReadDto {
                session_id: output.session_id.as_str().to_owned(),
                cursor: output.sequence.saturating_add(1),
                output: output.data_base64,
                closed,
            },
            None => ManagedRuntimeTerminalReadDto {
                session_id: session_id.to_owned(),
                cursor,
                output: String::new(),
                closed,
            },
        })
    }

    fn write_terminal(
        &self,
        session_id: &str,
        input: &str,
    ) -> Result<(), ManagedRuntimeCommandError> {
        if input.len() > 128 * 1024 {
            return Err(ManagedRuntimeCommandError::new(
                "managed_runtime_terminal_input_invalid",
                "The terminal input is too large.",
                false,
            ));
        }
        self.terminal(session_id)?
            .write_input(input.as_bytes())
            .map_err(terminal_command_error)
    }

    fn resize_terminal(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), ManagedRuntimeCommandError> {
        self.terminal(session_id)?
            // Pixel dimensions are provider-internal and fixed at the
            // command boundary; callers can only request terminal cells.
            .resize(cols, rows, 0, 0)
            .map_err(terminal_command_error)
    }

    fn close_terminal(&self, session_id: &str) -> Result<(), ManagedRuntimeCommandError> {
        let session = self.terminal(session_id)?;
        session.cancel().map_err(terminal_command_error)?;
        self.terminal_sessions
            .lock()
            .map_err(|_| state_error())?
            .remove(session_id);
        Ok(())
    }

    /// Backend-only insertion point for the Claude provider session manager.
    /// A session is accepted only with the exact opaque id produced by the
    /// provider PTY implementation.
    pub(crate) fn track_claude_terminal(
        &self,
        session: ClaudeTerminalSession,
    ) -> Result<String, ManagedRuntimeCommandError> {
        let id = session.id().as_str().to_owned();
        self.terminal_sessions
            .lock()
            .map_err(|_| state_error())?
            .insert(id.clone(), Arc::new(session));
        Ok(id)
    }

    /// Starts a provider-owned Claude PTY only after its caller has obtained a
    /// sealed package selection and an active, matching profile. The session
    /// itself remains behind the backend command relay.
    pub(crate) fn start_claude_terminal(
        &self,
        runtime: &ClaudeCodeSubscriptionRuntime,
        package: &RuntimePackageSelection,
        profile: &crate::agent_accounts::runtime_profile::RuntimeProfile,
        spec: ClaudeTerminalLaunchSpec,
    ) -> Result<String, ManagedRuntimeCommandError> {
        let session = runtime
            .start_terminal(package, profile, spec)
            .map_err(terminal_command_error)?;
        self.track_claude_terminal(session)
    }
}

impl Default for ManagedRuntimeControlPlane {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct ManagedRuntimeSupervisorHttpBridge;

impl opencode::OpenCodeSupervisorHttpBridge for ManagedRuntimeSupervisorHttpBridge {
    fn launch_authenticated(
        &self,
        supervisor: &ManagedRuntimeSupervisor,
        package: &RuntimePackageSelection,
        profile: &crate::agent_accounts::runtime_profile::RuntimeProfile,
        spec: crate::agents::managed_runtime::ManagedRuntimeLaunchSpec,
        address: std::net::SocketAddr,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let (handle, password) = supervisor
            .launch_opencode_authenticated(
                package,
                profile,
                spec,
                ManagedRuntimeCancellation::new(),
            )
            .map_err(|error| {
                NativeRuntimeError::new(
                    NativeErrorCode::ProviderUnavailable,
                    format!(
                        "managed OpenCode runtime launch failed: {}",
                        error.code().as_str()
                    ),
                    true,
                )
            })?;
        cancellation.checkpoint()?;
        let password = opencode::OpenCodeServerPassword::new(password)?;
        opencode::ManagedOpenCodeServerSession::new(handle, address, password)
            .map(|session| Box::new(session) as Box<dyn OpenCodeServerSession>)
    }
}

struct ManagedRuntimeAccountProvider {
    control_plane: Arc<ManagedRuntimeControlPlane>,
    spec: ManagedProductSpec,
}

impl AgentAccountProvider for ManagedRuntimeAccountProvider {
    fn registration(&self) -> AgentProviderRegistration {
        AgentProviderRegistration {
            provider: self.spec.provider,
            product: self.spec.product,
            harness: crate::agents::AgentHarness::Alfred,
            auth_method: self.spec.product.auth_method(),
            custody_mode: self.spec.product.custody_mode(),
            managed_runtime_id: Some(self.spec.runtime_id),
            managed_runtime_version: Some(self.spec.runtime_version.into()),
            // Registration is visible for diagnostics, but remains blocked;
            // capability gates are still checked independently by commands.
            gate_code: Some(self.spec.primary_gate_code.into()),
        }
    }

    fn start_authorization(&self) -> Result<ProviderAuthorizationStart, AgentProviderError> {
        Err(self.blocked_provider_error())
    }

    fn complete_authorization<'a>(
        &'a self,
        _attempt: AuthorizationAttempt,
    ) -> ProviderFuture<'a, ProviderAccountGrant> {
        Box::pin(async move { Err(self.blocked_provider_error()) })
    }

    fn refresh<'a>(
        &'a self,
        _account: &'a AgentAccount,
        _access: ProviderAccountAccess,
    ) -> ProviderFuture<'a, ProviderAccountAccess> {
        Box::pin(async move { Err(self.blocked_provider_error()) })
    }

    fn revoke<'a>(
        &'a self,
        _account: &'a AgentAccount,
        _access: ProviderAccountAccess,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move { Err(self.blocked_provider_error()) })
    }
}

impl ManagedRuntimeAccountProvider {
    fn blocked_provider_error(&self) -> AgentProviderError {
        let _ = &self.control_plane;
        AgentProviderError::new(
            self.spec.primary_gate_code,
            AgentProviderFailureKind::PolicyDenied,
        )
    }
}

/// Shared provider verifier implementation.  Provider modules own their
/// pinned manifests and call their own request builders; this type is the
/// only production adapter that can turn independent platform evidence into
/// a sealed package verification capability.
pub struct ManagedRuntimeTrustVerifier {
    platform: Arc<dyn RuntimePlatformPublisherVerifier>,
    opencode_package: Option<(RuntimePackageStore, PathBuf)>,
}

impl ManagedRuntimeTrustVerifier {
    pub fn new(platform: Arc<dyn RuntimePlatformPublisherVerifier>) -> Self {
        Self {
            platform,
            opencode_package: None,
        }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableRuntimePlatformPublisherVerifier))
    }

    pub fn with_opencode_package(
        mut self,
        store: RuntimePackageStore,
        package_root: impl Into<PathBuf>,
    ) -> Self {
        self.opencode_package = Some((store, package_root.into()));
        self
    }
}

impl Default for ManagedRuntimeTrustVerifier {
    fn default() -> Self {
        Self::unavailable()
    }
}

impl claude::ClaudePublisherVerifier for ManagedRuntimeTrustVerifier {
    fn verify(
        &self,
        request: ClaudePublisherVerificationRequest<'_>,
    ) -> Result<RuntimePackageVerification, ClaudePublisherVerificationError> {
        let Ok(target) = request
            .package_manifest
            .select_target(request.expectation.target())
        else {
            return Err(ClaudePublisherVerificationError);
        };
        if request.artifact.runtime_target != request.expectation.target()
            || target.executable.sha256 != request.artifact.sha256
            || target.publisher_verification.publisher != request.artifact.publisher
            || request.signing_key_fingerprint != claude::CLAUDE_CODE_RELEASE_SIGNING_FINGERPRINT
        {
            return Err(ClaudePublisherVerificationError);
        }
        verify_runtime_package_with_platform_evidence(
            request.package_root,
            request.package_manifest,
            request.expectation,
            request.publisher_release_manifest,
            request.detached_manifest_signature,
            self.platform.as_ref(),
        )
        .map_err(|_| ClaudePublisherVerificationError)
    }
}

impl CodexSdkPackageVerifier for ManagedRuntimeTrustVerifier {
    fn verify(
        &self,
        _request: CodexSdkVerifierRequest<'_>,
    ) -> Result<RuntimePackageVerification, CodexSdkPackageError> {
        // The provider slice intentionally does not yet expose a final
        // target manifest. Refuse the request until package assembly can
        // supply that pinned manifest; an invented empty target list must
        // never become sealed verification evidence.
        Err(CodexSdkPackageError::SealedVerificationUnavailable)
    }
}

impl OpenCodePackageVerifier for ManagedRuntimeTrustVerifier {
    fn select_verified_active(
        &self,
        manifest: &RuntimePackageManifest,
        expectation: &crate::agents::runtime_package::RuntimePackageExpectation,
        release: opencode::OpenCodeReleaseArtifact,
    ) -> Result<RuntimePackageSelection, crate::agents::native::NativeRuntimeError> {
        if expectation.product() != OPENCODE_PRODUCT
            || expectation.runtime_id()
                != crate::agent_accounts::models::ManagedRuntimeId::OpencodeServer
            || expectation.runtime_version() != opencode::OPENCODE_RUNTIME_VERSION
            || manifest != &opencode::package_manifest()
        {
            return Err(native_package_gate());
        }
        let Some((store, package_root)) = self.opencode_package.as_ref() else {
            return Err(native_package_gate());
        };
        let target = manifest
            .select_target(expectation.target())
            .map_err(|_| native_package_gate())?;
        if target.executable.sha256 != release.executable_sha256 {
            return Err(native_package_gate());
        }
        let verification = verify_runtime_package_with_platform_evidence(
            package_root,
            manifest,
            expectation,
            &[],
            &[],
            self.platform.as_ref(),
        )
        .map_err(|_| native_package_gate())?;
        store
            .stage_and_activate(package_root, &verification, None)
            .map_err(|_| native_package_gate())?;
        store
            .select_active(&verification)
            .map_err(|_| native_package_gate())
    }
}

fn native_package_gate() -> crate::agents::native::NativeRuntimeError {
    crate::agents::native::NativeRuntimeError::new(
        crate::agents::native::NativeErrorCode::ProviderUnavailable,
        "managed runtime package verification is unavailable",
        false,
    )
}

fn managed_products() -> Vec<ManagedProductSpec> {
    vec![
        ManagedProductSpec {
            provider: AgentProvider::ClaudeCode,
            product: CLAUDE_PRODUCT,
            runtime_id: CLAUDE_PRODUCT.managed_runtime().expect("pinned runtime"),
            runtime_version: claude::CLAUDE_CODE_RUNTIME_VERSION,
            connection_kind: "terminal",
            gate_codes: claude::subscription_release_gates()
                .into_iter()
                .filter_map(|gate| match gate.status {
                    crate::agents::native::CapabilityReportStatus::Supported => None,
                    crate::agents::native::CapabilityReportStatus::Unsupported
                    | crate::agents::native::CapabilityReportStatus::Blocked => {
                        Some(gate.evidence.into())
                    }
                })
                .collect(),
            primary_gate_code: claude::COMMERCIAL_TERMS_BLOCKED_CODE,
        },
        ManagedProductSpec {
            provider: AgentProvider::Codex,
            product: CODEX_PRODUCT,
            runtime_id: CODEX_PRODUCT.managed_runtime().expect("pinned runtime"),
            runtime_version: codex::CODEX_SDK_RUNTIME_VERSION,
            connection_kind: "browser",
            gate_codes: codex::codex_sdk_release_gates()
                .iter()
                .map(|gate| gate.evidence.into())
                .collect(),
            primary_gate_code: codex::PUBLIC_CAPABILITY_AUDIT_BLOCKER,
        },
        ManagedProductSpec {
            provider: AgentProvider::Opencode,
            product: OPENCODE_PRODUCT,
            runtime_id: OPENCODE_PRODUCT.managed_runtime().expect("pinned runtime"),
            runtime_version: opencode::OPENCODE_RUNTIME_VERSION,
            connection_kind: "api_key",
            gate_codes: opencode::native_release_gate()
                .blockers
                .iter()
                .map(|(code, _)| (*code).into())
                .collect(),
            primary_gate_code: opencode::COMMERCIAL_GATE_CODE,
        },
    ]
}

fn product_spec(
    provider_id: &str,
    product_id: &str,
) -> Result<ManagedProductSpec, ManagedRuntimeCommandError> {
    let provider = AgentProvider::from_str(provider_id).ok_or_else(|| {
        ManagedRuntimeCommandError::new(
            "provider_not_found",
            "The managed runtime provider is unknown.",
            false,
        )
    })?;
    let product = product_id.parse::<AgentProductId>().map_err(|_| {
        ManagedRuntimeCommandError::new(
            "product_not_found",
            "The managed runtime product is unknown.",
            false,
        )
    })?;
    managed_products()
        .into_iter()
        .find(|spec| spec.provider == provider && spec.product == product)
        .ok_or_else(|| {
            ManagedRuntimeCommandError::new(
                "managed_runtime_product_not_supported",
                "That provider/product is not a managed runtime product.",
                false,
            )
        })
}

fn valid_terminal_session_id(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("claude_terminal_") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn state_error() -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        "managed_runtime_state_unavailable",
        "Managed runtime state is temporarily unavailable.",
        true,
    )
}

fn package_command_error(
    error: RuntimePackageError,
    recoverable: bool,
) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        error.code().as_str(),
        "Managed runtime package storage is unavailable.",
        recoverable,
    )
}

fn profile_command_error(code: String, recoverable: bool) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        code,
        "Managed runtime profile storage is unavailable.",
        recoverable,
    )
}

fn terminal_command_error(error: ClaudeTerminalError) -> ManagedRuntimeCommandError {
    ManagedRuntimeCommandError::new(
        error.code().as_str(),
        "Managed runtime terminal operation failed.",
        true,
    )
}

/// Tauri command: enumerate exactly the managed provider products known to
/// this release.  It never probes PATH or a provider CLI.
#[tauri::command]
pub fn list_managed_runtime_products(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
) -> Result<Vec<ManagedRuntimeProductDto>, ManagedRuntimeCommandError> {
    state.inner().list_products()
}

#[tauri::command]
pub fn prepare_managed_runtime_product(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
) -> Result<ManagedRuntimeProductDto, ManagedRuntimeCommandError> {
    state.inner().prepare_product(&provider_id, &product_id)
}

#[tauri::command]
pub fn start_managed_runtime_connection(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
) -> Result<ManagedRuntimeConnectionStartDto, ManagedRuntimeCommandError> {
    state.inner().start_connection(&provider_id, &product_id)
}

#[tauri::command]
pub fn connect_managed_runtime_api_key(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
    api_key: AgentApiKeySecret,
) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
    state
        .inner()
        .connect_api_key(&provider_id, &product_id, api_key.into_zeroizing())
}

#[tauri::command]
pub fn managed_runtime_connection_status(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    provider_id: String,
    product_id: String,
) -> Result<ManagedRuntimeConnectionStatusDto, ManagedRuntimeCommandError> {
    state.inner().connection_status(&provider_id, &product_id)
}

#[tauri::command]
pub fn read_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
    cursor: u64,
) -> Result<ManagedRuntimeTerminalReadDto, ManagedRuntimeCommandError> {
    state.inner().read_terminal(&session_id, cursor)
}

#[tauri::command]
pub fn write_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
    input: String,
) -> Result<(), ManagedRuntimeCommandError> {
    state.inner().write_terminal(&session_id, &input)
}

#[tauri::command]
pub fn resize_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), ManagedRuntimeCommandError> {
    state.inner().resize_terminal(&session_id, cols, rows)
}

#[tauri::command]
pub fn close_managed_runtime_terminal(
    state: tauri::State<'_, Arc<ManagedRuntimeControlPlane>>,
    session_id: String,
) -> Result<(), ManagedRuntimeCommandError> {
    state.inner().close_terminal(&session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_projection_is_exact_and_fail_closed_before_setup() {
        let control_plane = ManagedRuntimeControlPlane::new();
        let products = control_plane.list_products().expect("product projection");
        assert_eq!(
            products
                .iter()
                .map(|product| product.product_id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude_code_subscription", "chatgpt_codex", "opencode_go"]
        );
        assert!(products.iter().all(|product| {
            !product.connect_available
                && product.install_state == "blocked"
                && product
                    .gate_codes
                    .iter()
                    .any(|code| code == "managed_runtime_storage_unavailable")
        }));
    }

    #[test]
    fn provider_product_pairs_cannot_cross_route() {
        let control_plane = ManagedRuntimeControlPlane::new();
        let error = control_plane
            .prepare_product("claude_code", "chatgpt_codex")
            .expect_err("cross-provider product must be rejected");
        assert_eq!(error.code, "managed_runtime_product_not_supported");
    }
}
