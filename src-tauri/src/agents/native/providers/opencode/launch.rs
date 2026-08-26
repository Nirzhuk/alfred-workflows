//! OpenCode-specific use of the sealed profile/package/supervisor substrate.

use super::package::OPENCODE_RUNTIME_VERSION;
use super::transport::{HttpOpenCodeApi, OpenCodeApi, OpenCodeServerPassword};
use crate::agent_accounts::models::{AgentProductId, ManagedRuntimeId};
use crate::agent_accounts::runtime_profile::{
    RuntimeProfile, RuntimeProfileBinding, RuntimeProfileRef, RuntimeProfileStore,
};
use crate::agents::managed_runtime::{
    ManagedRuntimeCancellation, ManagedRuntimeHandle, ManagedRuntimeLaunchSpec,
    ManagedRuntimeLifecycle, ManagedRuntimeSupervisor, RuntimeReadinessProbe, RuntimeShutdownHook,
    RuntimeStdoutPolicy,
};
use crate::agents::native::{NativeCancellation, NativeErrorCode, NativeRuntimeError};
use crate::agents::runtime_package::RuntimePackageSelection;
use crate::agents::OpaqueAgentAccountRef;
use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_OPERATION_DEADLINE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodeServerState {
    Active,
    Exited,
    Failed,
}

/// A live authenticated server. The API capability and process lifecycle stay
/// backend-only and are dropped together.
pub trait OpenCodeServerSession: Send + Sync {
    fn api(&self) -> &dyn OpenCodeApi;
    fn state(&self) -> OpenCodeServerState;
    fn stop(&self) -> Result<(), NativeRuntimeError>;
}

/// Shared hook required to join the supervisor's generated password to the
/// fixed-loopback HTTP client.
///
/// A production implementation must call `supervisor.launch` with the exact
/// arguments supplied here and return the generated Basic-auth password only
/// through [`OpenCodeServerPassword`]. It must never log, serialize, persist,
/// or expose that password. The existing supervisor does not yet expose such a
/// capability, so production registration remains blocked.
pub trait OpenCodeSupervisorHttpBridge: Send + Sync {
    fn launch_authenticated(
        &self,
        supervisor: &ManagedRuntimeSupervisor,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        spec: ManagedRuntimeLaunchSpec,
        address: SocketAddr,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError>;
}

pub struct ManagedOpenCodeServerSession {
    handle: ManagedRuntimeHandle,
    api: HttpOpenCodeApi,
}

impl ManagedOpenCodeServerSession {
    /// Called only by the trusted supervisor bridge after it has launched the
    /// verified package and received the matching generated password.
    pub(crate) fn new(
        handle: ManagedRuntimeHandle,
        address: SocketAddr,
        password: OpenCodeServerPassword,
    ) -> Result<Self, NativeRuntimeError> {
        match HttpOpenCodeApi::new(address, password) {
            Ok(api) => Ok(Self { handle, api }),
            Err(error) => {
                let _ = handle.stop();
                Err(error)
            }
        }
    }
}

impl OpenCodeServerSession for ManagedOpenCodeServerSession {
    fn api(&self) -> &dyn OpenCodeApi {
        &self.api
    }

    fn state(&self) -> OpenCodeServerState {
        match self.handle.snapshot().lifecycle {
            ManagedRuntimeLifecycle::Starting | ManagedRuntimeLifecycle::Ready => {
                OpenCodeServerState::Active
            }
            ManagedRuntimeLifecycle::Exited | ManagedRuntimeLifecycle::Stopping => {
                OpenCodeServerState::Exited
            }
            ManagedRuntimeLifecycle::Failed => OpenCodeServerState::Failed,
        }
    }

    fn stop(&self) -> Result<(), NativeRuntimeError> {
        self.handle
            .stop()
            .map(|_| ())
            .map_err(|_| runtime_unavailable())
    }
}

pub trait OpenCodeServerProvider: Send + Sync {
    fn create_and_launch(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        repository: &Path,
        cancellation: &NativeCancellation,
    ) -> Result<(RuntimeProfileRef, Box<dyn OpenCodeServerSession>), NativeRuntimeError>;

    fn launch_existing(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        profile_ref: &RuntimeProfileRef,
        repository: &Path,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError>;

    fn purge_profile(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        profile_ref: &RuntimeProfileRef,
    ) -> Result<(), NativeRuntimeError>;
}

/// Constructible production adapter once the two sealed shared hooks provide a
/// verified package selection and an authenticated supervisor bridge.
pub struct OpenCodeManagedServerFactory {
    package: RuntimePackageSelection,
    profiles: RuntimeProfileStore,
    supervisor: ManagedRuntimeSupervisor,
    bridge: Arc<dyn OpenCodeSupervisorHttpBridge>,
}

impl OpenCodeManagedServerFactory {
    pub fn new(
        package: RuntimePackageSelection,
        profiles: RuntimeProfileStore,
        supervisor: ManagedRuntimeSupervisor,
        bridge: Arc<dyn OpenCodeSupervisorHttpBridge>,
    ) -> Result<Self, NativeRuntimeError> {
        let expectation = package.expectation();
        if expectation.product() != AgentProductId::OpencodeGo
            || expectation.runtime_id() != ManagedRuntimeId::OpencodeServer
            || expectation.runtime_version() != OPENCODE_RUNTIME_VERSION
        {
            return Err(runtime_unavailable());
        }
        Ok(Self {
            package,
            profiles,
            supervisor,
            bridge,
        })
    }

    fn binding(
        account_ref: &OpaqueAgentAccountRef,
    ) -> Result<RuntimeProfileBinding, NativeRuntimeError> {
        RuntimeProfileBinding::new(
            account_ref,
            AgentProductId::OpencodeGo,
            ManagedRuntimeId::OpencodeServer,
            OPENCODE_RUNTIME_VERSION,
        )
        .map_err(|_| profile_unavailable())
    }

    fn launch_profile(
        &self,
        profile: &RuntimeProfile,
        repository: &Path,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let launch = OpenCodeLaunchSpec::allocate(repository, DEFAULT_OPERATION_DEADLINE)?;
        let address = launch.address();
        let managed = launch.into_managed()?;
        self.bridge.launch_authenticated(
            &self.supervisor,
            &self.package,
            profile,
            managed,
            address,
            cancellation,
        )
    }
}

impl OpenCodeServerProvider for OpenCodeManagedServerFactory {
    fn create_and_launch(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        repository: &Path,
        cancellation: &NativeCancellation,
    ) -> Result<(RuntimeProfileRef, Box<dyn OpenCodeServerSession>), NativeRuntimeError> {
        let binding = Self::binding(account_ref)?;
        let profile = self
            .profiles
            .create(&binding)
            .map_err(|_| profile_unavailable())?;
        let profile_ref = profile.profile_ref().clone();
        match self.launch_profile(&profile, repository, cancellation) {
            Ok(server) => Ok((profile_ref, server)),
            Err(error) => {
                let _ = self.profiles.purge(&profile_ref, &binding);
                Err(error)
            }
        }
    }

    fn launch_existing(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        profile_ref: &RuntimeProfileRef,
        repository: &Path,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeServerSession>, NativeRuntimeError> {
        let binding = Self::binding(account_ref)?;
        let profile = self
            .profiles
            .open(profile_ref, &binding)
            .map_err(|_| profile_unavailable())?;
        self.launch_profile(&profile, repository, cancellation)
    }

    fn purge_profile(
        &self,
        account_ref: &OpaqueAgentAccountRef,
        profile_ref: &RuntimeProfileRef,
    ) -> Result<(), NativeRuntimeError> {
        let binding = Self::binding(account_ref)?;
        self.profiles
            .purge(profile_ref, &binding)
            .map_err(|_| profile_unavailable())
    }
}

pub struct OpenCodeLaunchSpec {
    address: SocketAddr,
    repository: PathBuf,
    args: Vec<String>,
    environment: BTreeMap<String, String>,
    deadline: Duration,
}

impl OpenCodeLaunchSpec {
    pub fn allocate(repository: &Path, deadline: Duration) -> Result<Self, NativeRuntimeError> {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| runtime_unavailable())?;
        let address = listener.local_addr().map_err(|_| runtime_unavailable())?;
        drop(listener);
        Self::new(repository, address, deadline)
    }

    pub fn new(
        repository: &Path,
        address: SocketAddr,
        deadline: Duration,
    ) -> Result<Self, NativeRuntimeError> {
        if !repository.is_absolute()
            || address.ip() != std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)
            || address.port() == 0
            || deadline.is_zero()
        {
            return Err(invalid_launch());
        }
        let config = serde_json::json!({
            "autoupdate": false,
            "share": "disabled",
            "server": {"hostname": "127.0.0.1", "mdns": false, "cors": []},
            "permission": {"*": "deny"}
        });
        let environment = BTreeMap::from([
            ("OPENCODE_CONFIG_CONTENT".into(), config.to_string()),
            ("OPENCODE_DISABLE_AUTOUPDATE".into(), "true".into()),
            ("OPENCODE_DISABLE_PROJECT_CONFIG".into(), "true".into()),
        ]);
        Ok(Self {
            address,
            repository: repository.to_path_buf(),
            args: vec![
                "serve".into(),
                "--hostname=127.0.0.1".into(),
                format!("--port={}", address.port()),
                "--mdns=false".into(),
            ],
            environment,
            deadline,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    pub fn repository(&self) -> &Path {
        &self.repository
    }

    pub fn into_managed(self) -> Result<ManagedRuntimeLaunchSpec, NativeRuntimeError> {
        let readiness = RuntimeReadinessProbe::opencode_authenticated_http_loopback(self.address)
            .map_err(|_| invalid_launch())?;
        Ok(ManagedRuntimeLaunchSpec::new(
            self.args,
            readiness,
            RuntimeShutdownHook::CloseStdin,
            RuntimeStdoutPolicy::LogsDropOldest,
        )
        .with_working_directory(self.repository)
        .with_environment(self.environment)
        .with_startup_timeout(STARTUP_TIMEOUT)
        .with_shutdown_timeout(SHUTDOWN_TIMEOUT)
        .with_runtime_deadline(self.deadline))
    }
}

impl fmt::Debug for OpenCodeLaunchSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenCodeLaunchSpec")
            .field("address", &self.address)
            .field("repository", &"[REDACTED]")
            .field("args", &self.args)
            .field(
                "environment_keys",
                &self.environment.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Helper for the future shared bridge. Keeping the exact supervisor call here
/// makes the required package/profile/cancellation boundary unambiguous; the
/// bridge still needs a shared API that returns the generated password.
pub(crate) fn launch_with_supervisor(
    supervisor: &ManagedRuntimeSupervisor,
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
    spec: ManagedRuntimeLaunchSpec,
) -> Result<ManagedRuntimeHandle, NativeRuntimeError> {
    supervisor
        .launch(package, profile, spec, ManagedRuntimeCancellation::new())
        .map_err(|_| runtime_unavailable())
}

fn invalid_launch() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidRequest,
        "OpenCode managed server launch contract is invalid",
        false,
    )
}

fn profile_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::AccountUnavailable,
        "OpenCode managed profile is unavailable",
        false,
    )
}

fn runtime_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        "OpenCode managed runtime is unavailable",
        true,
    )
}
