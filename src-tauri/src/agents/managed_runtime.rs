//! Supervised lifecycle for verified Alfred-managed runtime sidecars.
//!
//! This module owns process isolation and cleanup only. Provider protocols,
//! account operations, downloads, and runtime registration live elsewhere.

use super::native::{contains_diagnostic_secret, redact_text};
use super::runtime_package::RuntimePackageSelection;
use crate::agent_accounts::runtime_profile::{
    RuntimeProfile, RuntimeProfileLifecycle, RuntimeProfileSupervisorLease,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

const MAX_ARGS: usize = 128;
const MAX_ARG_BYTES: usize = 64 * 1024;
const MAX_ENVIRONMENT_ENTRIES: usize = 32;
const MAX_ENVIRONMENT_KEY_BYTES: usize = 128;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 16 * 1024;
const MAX_PATH_EXTENSION_ENTRIES: usize = 24;
const MAX_PATH_EXTENSION_BYTES: usize = 16 * 1024;
const MAX_STDOUT_FRAME_BYTES: usize = 256 * 1024;
const MAX_STDERR_LINE_BYTES: usize = 64 * 1024;
const MAX_STDERR_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_BUFFERED_STDOUT_FRAMES: usize = 256;
const OUTPUT_CHANNEL_CAPACITY: usize = 64;
const MAX_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const MONITOR_TICK: Duration = Duration::from_millis(10);
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const PROCESS_TERM_GRACE: Duration = Duration::from_millis(150);
const HEALTH_IO_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_HEALTH_RESPONSE_BYTES: usize = 8 * 1024;
const MAX_HEALTH_BODY_BYTES: usize = 512;
const OPENCODE_HEALTH_PATH: &str = "/global/health";
const OPENCODE_SERVER_PASSWORD_ENV: &str = "OPENCODE_SERVER_PASSWORD";
const OPENCODE_SERVER_USERNAME: &str = "opencode";
const LEGACY_READY_PREFIX: &[u8] = b"ALFRED_RUNTIME_READY ";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ManagedRuntimeErrorCode {
    InvalidSelection,
    InvalidLaunch,
    EnvironmentRejected,
    DuplicateLaunch,
    SpawnFailed,
    StartupTimeout,
    StartupCancelled,
    StartupFailed,
    ReadinessPortOccupied,
    ReadinessHandshakeFailed,
    OutputLimitExceeded,
    DeadlineExceeded,
    RuntimeCrashed,
    RuntimeNotActive,
    IoFailed,
    StopTimedOut,
}

impl ManagedRuntimeErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidSelection => "managed_runtime_selection_invalid",
            Self::InvalidLaunch => "managed_runtime_launch_invalid",
            Self::EnvironmentRejected => "managed_runtime_environment_rejected",
            Self::DuplicateLaunch => "managed_runtime_duplicate_launch",
            Self::SpawnFailed => "managed_runtime_spawn_failed",
            Self::StartupTimeout => "managed_runtime_startup_timeout",
            Self::StartupCancelled => "managed_runtime_startup_cancelled",
            Self::StartupFailed => "managed_runtime_startup_failed",
            Self::ReadinessPortOccupied => "managed_runtime_readiness_port_occupied",
            Self::ReadinessHandshakeFailed => "managed_runtime_readiness_handshake_failed",
            Self::OutputLimitExceeded => "managed_runtime_output_limit_exceeded",
            Self::DeadlineExceeded => "managed_runtime_deadline_exceeded",
            Self::RuntimeCrashed => "managed_runtime_crashed",
            Self::RuntimeNotActive => "managed_runtime_not_active",
            Self::IoFailed => "managed_runtime_io_failed",
            Self::StopTimedOut => "managed_runtime_stop_timed_out",
        }
    }
}

impl fmt::Debug for ManagedRuntimeErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ManagedRuntimeErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

pub struct ManagedRuntimeError {
    code: ManagedRuntimeErrorCode,
}

impl ManagedRuntimeError {
    fn new(code: ManagedRuntimeErrorCode) -> Self {
        Self { code }
    }

    pub fn code(&self) -> ManagedRuntimeErrorCode {
        self.code
    }
}

impl fmt::Debug for ManagedRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl fmt::Display for ManagedRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for ManagedRuntimeError {}

pub type ManagedRuntimeResult<T> = Result<T, ManagedRuntimeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRuntimeLifecycle {
    Starting,
    Ready,
    Stopping,
    Exited,
    Failed,
}

impl ManagedRuntimeLifecycle {
    fn terminal(self) -> bool {
        matches!(self, Self::Exited | Self::Failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRuntimeTerminationKind {
    CleanExit,
    GracefulStop,
    ForcedStop,
    Cancelled,
    DeadlineExceeded,
    StartupFailed,
    Crash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedRuntimeTermination {
    pub kind: ManagedRuntimeTerminationKind,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedRuntimeSnapshot {
    pub lifecycle: ManagedRuntimeLifecycle,
    pub termination: Option<ManagedRuntimeTermination>,
    pub failure: Option<ManagedRuntimeErrorCode>,
}

#[derive(Clone)]
pub struct ManagedRuntimeCancellation {
    cancelled: Arc<AtomicBool>,
}

impl ManagedRuntimeCancellation {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Default for ManagedRuntimeCancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ManagedRuntimeCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ManagedRuntimeCancellation")
    }
}

#[derive(Clone)]
pub enum RuntimeReadinessProbe {
    StdoutLineEquals(String),
    OpenCodeAuthenticatedHttpLoopback(SocketAddr),
}

impl RuntimeReadinessProbe {
    pub fn stdout_line_equals(expected: impl Into<String>) -> ManagedRuntimeResult<Self> {
        let expected = expected.into();
        if expected.is_empty()
            || expected.len() > MAX_STDOUT_FRAME_BYTES
            || expected
                .chars()
                .any(|character| matches!(character, '\r' | '\n' | '\0'))
        {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        Ok(Self::StdoutLineEquals(expected))
    }

    /// Readiness for an unmodified `opencode serve` child. The supervisor
    /// preflights a free loopback port, generates the Basic-auth password,
    /// injects it only as `OPENCODE_SERVER_PASSWORD`, and requires an exact
    /// HTTP 200 health document containing the package-pinned runtime version.
    pub fn opencode_authenticated_http_loopback(address: SocketAddr) -> ManagedRuntimeResult<Self> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        Ok(Self::OpenCodeAuthenticatedHttpLoopback(address))
    }
}

impl fmt::Debug for RuntimeReadinessProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StdoutLineEquals(_) => formatter.write_str("StdoutLineEquals([REDACTED])"),
            Self::OpenCodeAuthenticatedHttpLoopback(address) => formatter
                .debug_tuple("OpenCodeAuthenticatedHttpLoopback")
                .field(address)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStdoutPolicy {
    TypedFramesFailClosed,
    LogsDropOldest,
}

#[derive(Clone)]
pub enum RuntimeShutdownHook {
    CloseStdin,
    StdinLine(String),
}

impl RuntimeShutdownHook {
    pub fn stdin_line(line: impl Into<String>) -> ManagedRuntimeResult<Self> {
        let line = line.into();
        if line.is_empty()
            || line.len() > MAX_STDOUT_FRAME_BYTES
            || line
                .chars()
                .any(|character| matches!(character, '\r' | '\n' | '\0'))
        {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        Ok(Self::StdinLine(line))
    }
}

impl fmt::Debug for RuntimeShutdownHook {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CloseStdin => formatter.write_str("CloseStdin"),
            Self::StdinLine(_) => formatter.write_str("StdinLine([REDACTED])"),
        }
    }
}

#[derive(Clone)]
pub struct ManagedRuntimeLaunchSpec {
    args: Vec<String>,
    working_directory: Option<PathBuf>,
    environment: BTreeMap<String, String>,
    path_extensions: Vec<PathBuf>,
    readiness: RuntimeReadinessProbe,
    shutdown: RuntimeShutdownHook,
    stdout_policy: RuntimeStdoutPolicy,
    startup_timeout: Duration,
    shutdown_timeout: Duration,
    runtime_deadline: Option<Duration>,
}

impl ManagedRuntimeLaunchSpec {
    pub fn new(
        args: Vec<String>,
        readiness: RuntimeReadinessProbe,
        shutdown: RuntimeShutdownHook,
        stdout_policy: RuntimeStdoutPolicy,
    ) -> Self {
        Self {
            args,
            working_directory: None,
            environment: BTreeMap::new(),
            path_extensions: Vec::new(),
            readiness,
            shutdown,
            stdout_policy,
            startup_timeout: Duration::from_secs(20),
            shutdown_timeout: Duration::from_secs(2),
            runtime_deadline: None,
        }
    }

    /// Provider adapters must set an explicit execution cwd. The profile-root
    /// fallback exists only for substrate compatibility and is not a suitable
    /// provider project directory.
    pub fn with_working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    pub fn with_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.environment = environment;
        self
    }

    /// Adds directories after the code-owned package/toolchain prefix. Raw
    /// `PATH` replacement remains forbidden. Validation is repeated at launch
    /// to catch directory replacement between construction and spawn.
    pub fn with_path_extensions(mut self, paths: Vec<PathBuf>) -> ManagedRuntimeResult<Self> {
        self.path_extensions = validate_path_extensions(&paths)?;
        Ok(self)
    }

    pub fn with_startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    pub fn with_runtime_deadline(mut self, deadline: Duration) -> Self {
        self.runtime_deadline = Some(deadline);
        self
    }
}

impl fmt::Debug for ManagedRuntimeLaunchSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedRuntimeLaunchSpec")
            .field("arg_count", &self.args.len())
            .field("has_working_directory", &self.working_directory.is_some())
            .field(
                "environment_keys",
                &self.environment.keys().collect::<Vec<_>>(),
            )
            .field("path_extension_count", &self.path_extensions.len())
            .field("readiness", &self.readiness)
            .field("shutdown", &self.shutdown)
            .field("stdout_policy", &self.stdout_policy)
            .field("startup_timeout", &self.startup_timeout)
            .field("shutdown_timeout", &self.shutdown_timeout)
            .field("has_runtime_deadline", &self.runtime_deadline.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RuntimeKey {
    runtime_id: crate::agent_accounts::models::ManagedRuntimeId,
    profile_ref: String,
}

struct SupervisorInner {
    entries: Mutex<HashMap<RuntimeKey, Arc<RuntimeEntry>>>,
}

impl Drop for SupervisorInner {
    fn drop(&mut self) {
        let entries = self
            .entries
            .get_mut()
            .map(|entries| entries.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for entry in &entries {
            entry.request_stop(StopReason::Requested);
        }
        for entry in entries {
            let _ = entry.wait_terminal(entry.shutdown_timeout + Duration::from_secs(2));
        }
    }
}

#[derive(Clone)]
pub struct ManagedRuntimeSupervisor {
    inner: Arc<SupervisorInner>,
}

impl ManagedRuntimeSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SupervisorInner {
                entries: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn launch(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        spec: ManagedRuntimeLaunchSpec,
        cancellation: ManagedRuntimeCancellation,
    ) -> ManagedRuntimeResult<ManagedRuntimeHandle> {
        if cancellation.is_cancelled() {
            return Err(runtime_error(ManagedRuntimeErrorCode::StartupCancelled));
        }
        validate_selection(package, profile)?;
        let selected_executable = package
            .verified_active_executable_path()
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidSelection))?;
        let executable = validate_executable(&selected_executable)?;
        let mut validated =
            validate_launch_spec(spec, profile, package.expectation().runtime_version())?;
        validated
            .sensitive_values
            .push(executable.to_string_lossy().into_owned());
        validated.sensitive_values.push(
            child_environment_path(&executable)
                .to_string_lossy()
                .into_owned(),
        );
        let startup_wait_budget =
            validated.startup_timeout + validated.shutdown_timeout + Duration::from_secs(2);
        let key = RuntimeKey {
            runtime_id: package.expectation().runtime_id(),
            profile_ref: profile.profile_ref().as_str().to_owned(),
        };
        let profile_lease = profile
            .acquire_supervisor_lease()
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidSelection))?;
        let entry = Arc::new(RuntimeEntry::new(
            validated.shutdown_timeout,
            validated.sensitive_values.clone(),
            profile_lease,
            validated.readiness.opencode_password().map(str::to_owned),
        ));
        {
            let mut entries = self
                .inner
                .entries
                .lock()
                .map_err(|_| runtime_error(ManagedRuntimeErrorCode::RuntimeNotActive))?;
            entries.retain(|_, candidate| !candidate.snapshot().lifecycle.terminal());
            if entries.contains_key(&key) {
                return Err(runtime_error(ManagedRuntimeErrorCode::DuplicateLaunch));
            }
            entries.insert(key.clone(), Arc::clone(&entry));
        }

        let spawn_result = spawn_runtime(&executable, profile, &validated);
        let (process, stdin, stdout, stderr) = match spawn_result {
            Ok(spawned) => spawned,
            Err(()) => {
                self.remove_entry(&key, &entry);
                return Err(runtime_error(ManagedRuntimeErrorCode::SpawnFailed));
            }
        };
        entry.process_id.store(process.child.id(), Ordering::SeqCst);
        match entry.stdin.lock() {
            Ok(mut slot) => *slot = Some(stdin),
            Err(_) => {
                self.remove_entry(&key, &entry);
                return Err(runtime_error(ManagedRuntimeErrorCode::RuntimeNotActive));
            }
        }

        let (sender, receiver) = mpsc::sync_channel(OUTPUT_CHANNEL_CAPACITY);
        let stdout_sender = sender.clone();
        let stderr_sender = sender;
        let stdout_thread = thread::spawn(move || {
            read_bounded_frames(
                stdout,
                StreamKind::Stdout,
                MAX_STDOUT_FRAME_BYTES,
                stdout_sender,
            )
        });
        let stderr_thread = thread::spawn(move || {
            read_bounded_frames(
                stderr,
                StreamKind::Stderr,
                MAX_STDERR_LINE_BYTES,
                stderr_sender,
            )
        });
        let weak_supervisor = Arc::downgrade(&self.inner);
        let monitor_entry = Arc::clone(&entry);
        thread::spawn(move || {
            monitor_runtime(
                process,
                monitor_entry,
                key,
                weak_supervisor,
                validated,
                cancellation,
                receiver,
                stdout_thread,
                stderr_thread,
            );
        });

        let startup_wait = validated_startup_wait(&entry, startup_wait_budget);
        match startup_wait {
            Ok(()) => Ok(ManagedRuntimeHandle {
                supervisor: Arc::clone(&self.inner),
                entry,
            }),
            Err(code) => {
                entry.request_stop(match code {
                    ManagedRuntimeErrorCode::StartupCancelled => StopReason::Cancelled,
                    ManagedRuntimeErrorCode::StartupTimeout => StopReason::StartupTimeout,
                    _ => StopReason::Requested,
                });
                let _ = entry.wait_terminal(entry.shutdown_timeout + Duration::from_secs(2));
                Err(runtime_error(code))
            }
        }
    }

    /// Launches an OpenCode server and transfers its per-process Basic-auth
    /// password directly to the trusted HTTP bridge.  The password is only
    /// available from the authenticated readiness path; callers cannot supply
    /// one, derive one from a runtime response, or retrieve it via a DTO.
    pub(crate) fn launch_opencode_authenticated(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        spec: ManagedRuntimeLaunchSpec,
        cancellation: ManagedRuntimeCancellation,
    ) -> ManagedRuntimeResult<(ManagedRuntimeHandle, String)> {
        let handle = self.launch(package, profile, spec, cancellation)?;
        let Some(password) = handle.take_opencode_password() else {
            let _ = handle.stop();
            return Err(runtime_error(
                ManagedRuntimeErrorCode::ReadinessHandshakeFailed,
            ));
        };
        Ok((handle, password))
    }

    fn remove_entry(&self, key: &RuntimeKey, expected: &Arc<RuntimeEntry>) {
        if let Ok(mut entries) = self.inner.entries.lock() {
            if entries
                .get(key)
                .is_some_and(|candidate| Arc::ptr_eq(candidate, expected))
            {
                entries.remove(key);
            }
        }
    }
}

impl Default for ManagedRuntimeSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ManagedRuntimeSupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active_count = self
            .inner
            .entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or_default();
        formatter
            .debug_struct("ManagedRuntimeSupervisor")
            .field("active_count", &active_count)
            .finish()
    }
}

pub struct ManagedRuntimeHandle {
    supervisor: Arc<SupervisorInner>,
    entry: Arc<RuntimeEntry>,
}

impl ManagedRuntimeHandle {
    pub fn snapshot(&self) -> ManagedRuntimeSnapshot {
        self.entry.snapshot()
    }

    pub fn process_id(&self) -> u32 {
        self.entry.process_id.load(Ordering::SeqCst)
    }

    /// Transfers the one-launch OpenCode Basic-auth password to the trusted
    /// provider bridge.  It is never serialized, logged, or exposed through a
    /// command DTO.  A caller that does not need this capability simply drops
    /// the handle and the password with it.
    pub(crate) fn take_opencode_password(&self) -> Option<String> {
        self.entry
            .opencode_password
            .lock()
            .ok()
            .and_then(|mut password| password.take())
    }

    pub fn stop(&self) -> ManagedRuntimeResult<ManagedRuntimeSnapshot> {
        self.entry.request_stop(StopReason::Requested);
        self.entry
            .wait_terminal(self.entry.shutdown_timeout + Duration::from_secs(2))
            .ok_or_else(|| runtime_error(ManagedRuntimeErrorCode::StopTimedOut))
    }

    pub fn wait_for_terminal(
        &self,
        timeout: Duration,
    ) -> ManagedRuntimeResult<ManagedRuntimeSnapshot> {
        self.entry
            .wait_terminal(timeout)
            .ok_or_else(|| runtime_error(ManagedRuntimeErrorCode::StopTimedOut))
    }

    /// Backend-only protocol frame access. Frames are newline-delimited,
    /// individually bounded, and retained in a fixed-size queue.
    pub(crate) fn read_stdout_frame(
        &self,
        timeout: Duration,
    ) -> ManagedRuntimeResult<Option<Vec<u8>>> {
        self.entry.read_stdout_frame(timeout)
    }

    /// Writes one bounded newline-delimited provider frame without shell
    /// parsing. Protocol adapters remain responsible for their typed payload.
    pub(crate) fn write_stdin_frame(&self, frame: &[u8]) -> ManagedRuntimeResult<()> {
        if frame.is_empty()
            || frame.len() > MAX_STDOUT_FRAME_BYTES
            || frame
                .iter()
                .any(|byte| matches!(*byte, b'\0' | b'\r' | b'\n'))
        {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        if self.snapshot().lifecycle != ManagedRuntimeLifecycle::Ready {
            return Err(runtime_error(ManagedRuntimeErrorCode::RuntimeNotActive));
        }
        let mut stdin = self
            .entry
            .stdin
            .lock()
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::IoFailed))?;
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| runtime_error(ManagedRuntimeErrorCode::RuntimeNotActive))?;
        stdin
            .write_all(frame)
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::IoFailed))
    }

    /// Safe diagnostic tail. Known paths and credential-shaped lines are
    /// removed before capture; the returned text is already redacted.
    pub fn redacted_stderr_tail(&self) -> String {
        self.entry
            .state
            .lock()
            .map(|state| state.stderr.text.clone())
            .unwrap_or_default()
    }
}

impl fmt::Debug for ManagedRuntimeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedRuntimeHandle")
            .field("lifecycle", &self.snapshot().lifecycle)
            .field("process_id", &self.process_id())
            .finish()
    }
}

impl Drop for ManagedRuntimeHandle {
    fn drop(&mut self) {
        self.entry.request_stop(StopReason::Requested);
        let _ = self
            .entry
            .wait_terminal(self.entry.shutdown_timeout + Duration::from_secs(2));
        let _ = &self.supervisor;
    }
}

struct ValidatedLaunchSpec {
    args: Vec<String>,
    working_directory: PathBuf,
    environment: BTreeMap<String, String>,
    path_extensions: Vec<PathBuf>,
    readiness: ValidatedReadiness,
    shutdown: RuntimeShutdownHook,
    stdout_policy: RuntimeStdoutPolicy,
    startup_timeout: Duration,
    shutdown_timeout: Duration,
    runtime_deadline: Option<Duration>,
    sensitive_values: Vec<String>,
}

enum ValidatedReadiness {
    StdoutLineEquals(String),
    OpenCodeAuthenticatedHttpLoopback {
        address: SocketAddr,
        expected_version: String,
        password: String,
    },
}

enum ReadinessFrame {
    NotControl,
    Ready { retain: bool },
}

enum HttpReadinessOutcome {
    NotListening,
    Ready,
    Rejected,
}

impl ValidatedReadiness {
    fn handle_frame(&self, frame: &[u8]) -> ReadinessFrame {
        match self {
            Self::StdoutLineEquals(expected) => {
                if frame == expected.as_bytes() {
                    ReadinessFrame::Ready { retain: true }
                } else {
                    ReadinessFrame::NotControl
                }
            }
            Self::OpenCodeAuthenticatedHttpLoopback { .. } => ReadinessFrame::NotControl,
        }
    }

    fn opencode_password(&self) -> Option<&str> {
        match self {
            Self::StdoutLineEquals(_) => None,
            Self::OpenCodeAuthenticatedHttpLoopback { password, .. } => Some(password),
        }
    }

    fn poll_http(&self) -> HttpReadinessOutcome {
        match self {
            Self::StdoutLineEquals(_) => HttpReadinessOutcome::NotListening,
            Self::OpenCodeAuthenticatedHttpLoopback {
                address,
                expected_version,
                password,
            } => authenticated_opencode_health_check(*address, expected_version, password),
        }
    }
}

fn authenticated_opencode_health_check(
    address: SocketAddr,
    expected_version: &str,
    password: &str,
) -> HttpReadinessOutcome {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, HEALTH_IO_TIMEOUT) else {
        return HttpReadinessOutcome::NotListening;
    };
    if stream.set_read_timeout(Some(HEALTH_IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(HEALTH_IO_TIMEOUT)).is_err()
    {
        return HttpReadinessOutcome::Rejected;
    }
    let credentials = BASE64_STANDARD.encode(format!("{OPENCODE_SERVER_USERNAME}:{password}"));
    let request = format!(
        "GET {OPENCODE_HEALTH_PATH} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Basic {credentials}\r\nAccept: application/json\r\nConnection: keep-alive\r\n\r\n"
    );
    if stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.flush())
        .is_err()
    {
        return HttpReadinessOutcome::Rejected;
    }
    let Ok(body) = read_framed_http_body(&mut stream) else {
        return HttpReadinessOutcome::Rejected;
    };
    let Ok(health) = serde_json::from_slice::<OpenCodeHealthResponse>(&body) else {
        return HttpReadinessOutcome::Rejected;
    };
    if health.healthy && health.version == expected_version {
        HttpReadinessOutcome::Ready
    } else {
        HttpReadinessOutcome::Rejected
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenCodeHealthResponse {
    healthy: bool,
    version: String,
}

enum HttpBodyFraming {
    ContentLength(usize),
    Chunked,
}

fn read_framed_http_body(stream: &mut TcpStream) -> Result<Vec<u8>, ()> {
    let mut response = Vec::new();
    let mut framing = None;
    let mut body_start = 0usize;
    let mut buffer = [0u8; 1024];
    loop {
        if let Some(current) = &framing {
            let body = &response[body_start..];
            match current {
                HttpBodyFraming::ContentLength(length) if body.len() >= *length => {
                    return Ok(body[..*length].to_vec());
                }
                HttpBodyFraming::Chunked => {
                    if let Some(decoded) = decode_complete_chunked_body(body)? {
                        return Ok(decoded);
                    }
                }
                HttpBodyFraming::ContentLength(_) => {}
            }
        }
        if response.len() >= MAX_HEALTH_RESPONSE_BYTES {
            return Err(());
        }
        let read = stream.read(&mut buffer).map_err(|_| ())?;
        if read == 0 {
            return Err(());
        }
        if response.len().saturating_add(read) > MAX_HEALTH_RESPONSE_BYTES {
            return Err(());
        }
        response.extend_from_slice(&buffer[..read]);
        if framing.is_none() {
            let Some(header_end) = find_bytes(&response, b"\r\n\r\n") else {
                continue;
            };
            body_start = header_end + 4;
            framing = Some(parse_http_headers(&response[..header_end])?);
        }
    }
}

fn parse_http_headers(headers: &[u8]) -> Result<HttpBodyFraming, ()> {
    let headers = std::str::from_utf8(headers).map_err(|_| ())?;
    let mut lines = headers.split("\r\n");
    if lines.next() != Some("HTTP/1.1 200 OK") {
        return Err(());
    }
    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(())?;
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(());
            }
            let length = value.parse::<usize>().map_err(|_| ())?;
            if length > MAX_HEALTH_BODY_BYTES {
                return Err(());
            }
            content_length = Some(length);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if !value.eq_ignore_ascii_case("chunked") {
                return Err(());
            }
            chunked = true;
        }
    }
    match (content_length, chunked) {
        (Some(length), false) => Ok(HttpBodyFraming::ContentLength(length)),
        (None, true) => Ok(HttpBodyFraming::Chunked),
        _ => Err(()),
    }
}

fn decode_complete_chunked_body(bytes: &[u8]) -> Result<Option<Vec<u8>>, ()> {
    let mut cursor = 0usize;
    let mut decoded = Vec::new();
    loop {
        let Some(line_end) = find_bytes(&bytes[cursor..], b"\r\n") else {
            return Ok(None);
        };
        let line_end = cursor + line_end;
        let size_text = std::str::from_utf8(&bytes[cursor..line_end]).map_err(|_| ())?;
        let size_text = size_text.split(';').next().ok_or(())?;
        let size = usize::from_str_radix(size_text, 16).map_err(|_| ())?;
        cursor = line_end + 2;
        if size == 0 {
            if bytes.len() < cursor + 2 {
                return Ok(None);
            }
            return if &bytes[cursor..cursor + 2] == b"\r\n" {
                Ok(Some(decoded))
            } else {
                Err(())
            };
        }
        if decoded.len().saturating_add(size) > MAX_HEALTH_BODY_BYTES {
            return Err(());
        }
        let Some(chunk_end) = cursor.checked_add(size) else {
            return Err(());
        };
        if bytes.len() < chunk_end + 2 {
            return Ok(None);
        }
        if &bytes[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err(());
        }
        decoded.extend_from_slice(&bytes[cursor..chunk_end]);
        cursor = chunk_end + 2;
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|candidate| candidate == needle)
}

fn generate_opencode_password() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

struct RuntimeEntry {
    process_id: AtomicU32,
    state: Mutex<RuntimeEntryState>,
    changed: Condvar,
    stdin: Mutex<Option<ChildStdin>>,
    stop_reason: AtomicU8,
    shutdown_timeout: Duration,
    sensitive_values: Vec<String>,
    opencode_password: Mutex<Option<String>>,
    _profile_lease: RuntimeProfileSupervisorLease,
}

struct RuntimeEntryState {
    lifecycle: ManagedRuntimeLifecycle,
    termination: Option<ManagedRuntimeTermination>,
    failure: Option<ManagedRuntimeErrorCode>,
    stdout: VecDeque<Vec<u8>>,
    stderr: BoundedText,
}

impl RuntimeEntry {
    fn new(
        shutdown_timeout: Duration,
        sensitive_values: Vec<String>,
        profile_lease: RuntimeProfileSupervisorLease,
        opencode_password: Option<String>,
    ) -> Self {
        Self {
            process_id: AtomicU32::new(0),
            state: Mutex::new(RuntimeEntryState {
                lifecycle: ManagedRuntimeLifecycle::Starting,
                termination: None,
                failure: None,
                stdout: VecDeque::new(),
                stderr: BoundedText::new(MAX_STDERR_CAPTURE_BYTES),
            }),
            changed: Condvar::new(),
            stdin: Mutex::new(None),
            stop_reason: AtomicU8::new(StopReason::None as u8),
            shutdown_timeout,
            sensitive_values,
            opencode_password: Mutex::new(opencode_password),
            _profile_lease: profile_lease,
        }
    }

    fn snapshot(&self) -> ManagedRuntimeSnapshot {
        self.state
            .lock()
            .map(|state| ManagedRuntimeSnapshot {
                lifecycle: state.lifecycle,
                termination: state.termination,
                failure: state.failure,
            })
            .unwrap_or(ManagedRuntimeSnapshot {
                lifecycle: ManagedRuntimeLifecycle::Failed,
                termination: Some(ManagedRuntimeTermination {
                    kind: ManagedRuntimeTerminationKind::Crash,
                    exit_code: None,
                }),
                failure: Some(ManagedRuntimeErrorCode::RuntimeNotActive),
            })
    }

    fn set_ready(&self) {
        if let Ok(mut state) = self.state.lock() {
            if state.lifecycle == ManagedRuntimeLifecycle::Starting {
                state.lifecycle = ManagedRuntimeLifecycle::Ready;
                self.changed.notify_all();
            }
        }
    }

    fn set_stopping(&self) {
        if let Ok(mut state) = self.state.lock() {
            if !state.lifecycle.terminal() {
                state.lifecycle = ManagedRuntimeLifecycle::Stopping;
                self.changed.notify_all();
            }
        }
    }

    fn finish(
        &self,
        lifecycle: ManagedRuntimeLifecycle,
        termination: ManagedRuntimeTermination,
        failure: Option<ManagedRuntimeErrorCode>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state.lifecycle = lifecycle;
            state.termination = Some(termination);
            state.failure = failure;
            self.changed.notify_all();
        }
    }

    fn request_stop(&self, reason: StopReason) {
        let _ = self.stop_reason.compare_exchange(
            StopReason::None as u8,
            reason as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        self.changed.notify_all();
    }

    fn requested_stop(&self) -> StopReason {
        StopReason::from_u8(self.stop_reason.load(Ordering::SeqCst))
    }

    fn wait_terminal(&self, timeout: Duration) -> Option<ManagedRuntimeSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut state = self.state.lock().ok()?;
        loop {
            if state.lifecycle.terminal() {
                return Some(ManagedRuntimeSnapshot {
                    lifecycle: state.lifecycle,
                    termination: state.termination,
                    failure: state.failure,
                });
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self.changed.wait_timeout(state, remaining).ok()?;
            state = next;
            if result.timed_out() && !state.lifecycle.terminal() {
                return None;
            }
        }
    }

    fn read_stdout_frame(&self, timeout: Duration) -> ManagedRuntimeResult<Option<Vec<u8>>> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| runtime_error(ManagedRuntimeErrorCode::InvalidLaunch))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::RuntimeNotActive))?;
        loop {
            if let Some(frame) = state.stdout.pop_front() {
                return Ok(Some(frame));
            }
            if state.lifecycle.terminal() {
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let (next, result) = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| runtime_error(ManagedRuntimeErrorCode::RuntimeNotActive))?;
            state = next;
            if result.timed_out() && state.stdout.is_empty() {
                return Ok(None);
            }
        }
    }

    fn push_stdout(&self, frame: Vec<u8>, policy: RuntimeStdoutPolicy) -> bool {
        if let Ok(mut state) = self.state.lock() {
            if state.stdout.len() == MAX_BUFFERED_STDOUT_FRAMES {
                match policy {
                    RuntimeStdoutPolicy::TypedFramesFailClosed => return false,
                    RuntimeStdoutPolicy::LogsDropOldest => {
                        state.stdout.pop_front();
                    }
                }
            }
            state.stdout.push_back(frame);
            self.changed.notify_all();
            return true;
        }
        false
    }

    fn push_stderr(&self, line: &[u8]) {
        let text = String::from_utf8_lossy(line);
        let redacted = redact_diagnostic(&text, &self.sensitive_values);
        if let Ok(mut state) = self.state.lock() {
            state.stderr.push_line(&redacted);
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum StopReason {
    None = 0,
    Requested = 1,
    Cancelled = 2,
    StartupTimeout = 3,
    Deadline = 4,
}

impl StopReason {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Requested,
            2 => Self::Cancelled,
            3 => Self::StartupTimeout,
            4 => Self::Deadline,
            _ => Self::None,
        }
    }
}

struct BoundedText {
    text: String,
    max_bytes: usize,
}

impl BoundedText {
    fn new(max_bytes: usize) -> Self {
        Self {
            text: String::new(),
            max_bytes,
        }
    }

    fn push_line(&mut self, line: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(line);
        if self.text.len() <= self.max_bytes {
            return;
        }
        let mut start = self.text.len() - self.max_bytes;
        while start < self.text.len() && !self.text.is_char_boundary(start) {
            start += 1;
        }
        if let Some(newline) = self.text[start..].find('\n') {
            start += newline + 1;
        }
        self.text.drain(..start);
    }
}

#[derive(Clone, Copy)]
enum StreamKind {
    Stdout,
    Stderr,
}

enum StreamEvent {
    Frame(StreamKind, Vec<u8>),
    Oversized(StreamKind),
}

fn read_bounded_frames<R: Read>(
    mut reader: R,
    stream: StreamKind,
    max_frame_bytes: usize,
    sender: mpsc::SyncSender<StreamEvent>,
) {
    let mut pending = Vec::new();
    let mut discarding_oversized = false;
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => return,
        };
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                if discarding_oversized {
                    discarding_oversized = false;
                    continue;
                }
                if pending.last() == Some(&b'\r') {
                    pending.pop();
                }
                if sender
                    .send(StreamEvent::Frame(stream, std::mem::take(&mut pending)))
                    .is_err()
                {
                    return;
                }
                continue;
            }
            if discarding_oversized {
                continue;
            }
            pending.push(*byte);
            if pending.len() > max_frame_bytes {
                pending.clear();
                discarding_oversized = true;
                if sender.send(StreamEvent::Oversized(stream)).is_err() {
                    return;
                }
            }
        }
    }
    if !pending.is_empty() {
        let _ = sender.send(StreamEvent::Frame(stream, pending));
    }
}

#[allow(clippy::too_many_arguments)]
fn monitor_runtime(
    mut process: ProcessTree,
    entry: Arc<RuntimeEntry>,
    key: RuntimeKey,
    supervisor: Weak<SupervisorInner>,
    spec: ValidatedLaunchSpec,
    cancellation: ManagedRuntimeCancellation,
    receiver: mpsc::Receiver<StreamEvent>,
    stdout_thread: thread::JoinHandle<()>,
    stderr_thread: thread::JoinHandle<()>,
) {
    let started = Instant::now();
    let startup_deadline = started + spec.startup_timeout;
    let runtime_deadline = spec
        .runtime_deadline
        .and_then(|duration| started.checked_add(duration));
    let mut next_http_probe = started;
    loop {
        while let Ok(event) = receiver.try_recv() {
            if let Some(code) =
                handle_stream_event(&entry, Some(&spec.readiness), spec.stdout_policy, event)
            {
                let termination = if entry.snapshot().lifecycle == ManagedRuntimeLifecycle::Ready {
                    ManagedRuntimeTerminationKind::Crash
                } else {
                    ManagedRuntimeTerminationKind::StartupFailed
                };
                finish_failed_process(
                    &mut process,
                    &entry,
                    code,
                    termination,
                    spec.stdout_policy,
                    &receiver,
                    stdout_thread,
                    stderr_thread,
                );
                remove_monitored_entry(&supervisor, &key, &entry);
                return;
            }
        }

        if entry.snapshot().lifecycle == ManagedRuntimeLifecycle::Starting
            && Instant::now() >= next_http_probe
        {
            match spec.readiness.poll_http() {
                HttpReadinessOutcome::NotListening => {
                    next_http_probe = Instant::now() + MONITOR_TICK;
                }
                HttpReadinessOutcome::Ready => entry.set_ready(),
                HttpReadinessOutcome::Rejected => {
                    finish_failed_process(
                        &mut process,
                        &entry,
                        ManagedRuntimeErrorCode::ReadinessHandshakeFailed,
                        ManagedRuntimeTerminationKind::StartupFailed,
                        spec.stdout_policy,
                        &receiver,
                        stdout_thread,
                        stderr_thread,
                    );
                    remove_monitored_entry(&supervisor, &key, &entry);
                    return;
                }
            }
        }

        if cancellation.is_cancelled() {
            entry.request_stop(StopReason::Cancelled);
        }
        if entry.snapshot().lifecycle == ManagedRuntimeLifecycle::Starting
            && Instant::now() >= startup_deadline
        {
            entry.request_stop(StopReason::StartupTimeout);
        }
        if runtime_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            entry.request_stop(StopReason::Deadline);
        }

        let stop_reason = entry.requested_stop();
        if stop_reason != StopReason::None {
            stop_process(
                &mut process,
                &entry,
                stop_reason,
                &spec.shutdown,
                spec.shutdown_timeout,
                spec.stdout_policy,
                &receiver,
                stdout_thread,
                stderr_thread,
            );
            remove_monitored_entry(&supervisor, &key, &entry);
            return;
        }

        match process.try_wait() {
            Ok(Some(status)) => {
                let drain_failure = drain_and_join(
                    &entry,
                    Some(&spec.readiness),
                    spec.stdout_policy,
                    &receiver,
                    stdout_thread,
                    stderr_thread,
                );
                if let Some(failure) = drain_failure {
                    entry.finish(
                        ManagedRuntimeLifecycle::Failed,
                        ManagedRuntimeTermination {
                            kind: ManagedRuntimeTerminationKind::Crash,
                            exit_code: status.code(),
                        },
                        Some(failure),
                    );
                    remove_monitored_entry(&supervisor, &key, &entry);
                    return;
                }
                let was_ready = entry.snapshot().lifecycle == ManagedRuntimeLifecycle::Ready;
                if was_ready && status.success() {
                    entry.finish(
                        ManagedRuntimeLifecycle::Exited,
                        ManagedRuntimeTermination {
                            kind: ManagedRuntimeTerminationKind::CleanExit,
                            exit_code: status.code(),
                        },
                        None,
                    );
                } else {
                    let code = if was_ready {
                        ManagedRuntimeErrorCode::RuntimeCrashed
                    } else {
                        ManagedRuntimeErrorCode::StartupFailed
                    };
                    entry.finish(
                        ManagedRuntimeLifecycle::Failed,
                        ManagedRuntimeTermination {
                            kind: if was_ready {
                                ManagedRuntimeTerminationKind::Crash
                            } else {
                                ManagedRuntimeTerminationKind::StartupFailed
                            },
                            exit_code: status.code(),
                        },
                        Some(code),
                    );
                }
                remove_monitored_entry(&supervisor, &key, &entry);
                return;
            }
            Ok(None) => {}
            Err(_) => {
                finish_failed_process(
                    &mut process,
                    &entry,
                    ManagedRuntimeErrorCode::RuntimeCrashed,
                    ManagedRuntimeTerminationKind::Crash,
                    spec.stdout_policy,
                    &receiver,
                    stdout_thread,
                    stderr_thread,
                );
                remove_monitored_entry(&supervisor, &key, &entry);
                return;
            }
        }
        thread::sleep(MONITOR_TICK);
    }
}

fn handle_stream_event(
    entry: &RuntimeEntry,
    readiness: Option<&ValidatedReadiness>,
    stdout_policy: RuntimeStdoutPolicy,
    event: StreamEvent,
) -> Option<ManagedRuntimeErrorCode> {
    match event {
        StreamEvent::Frame(StreamKind::Stdout, frame) => {
            if frame.starts_with(LEGACY_READY_PREFIX) {
                return None;
            }
            let mut retain = true;
            if entry.snapshot().lifecycle == ManagedRuntimeLifecycle::Starting {
                match readiness
                    .map(|probe| probe.handle_frame(&frame))
                    .unwrap_or(ReadinessFrame::NotControl)
                {
                    ReadinessFrame::NotControl => {}
                    ReadinessFrame::Ready {
                        retain: retain_frame,
                    } => {
                        retain = retain_frame;
                        entry.set_ready();
                    }
                }
            }
            if !retain || entry.push_stdout(frame, stdout_policy) {
                None
            } else {
                Some(ManagedRuntimeErrorCode::OutputLimitExceeded)
            }
        }
        StreamEvent::Frame(StreamKind::Stderr, line) => {
            entry.push_stderr(&line);
            None
        }
        StreamEvent::Oversized(StreamKind::Stdout) => match stdout_policy {
            RuntimeStdoutPolicy::TypedFramesFailClosed => {
                Some(ManagedRuntimeErrorCode::OutputLimitExceeded)
            }
            RuntimeStdoutPolicy::LogsDropOldest => None,
        },
        StreamEvent::Oversized(StreamKind::Stderr) => {
            entry.push_stderr(b"[stderr line truncated]");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stop_process(
    process: &mut ProcessTree,
    entry: &RuntimeEntry,
    reason: StopReason,
    hook: &RuntimeShutdownHook,
    shutdown_timeout: Duration,
    stdout_policy: RuntimeStdoutPolicy,
    receiver: &mpsc::Receiver<StreamEvent>,
    stdout_thread: thread::JoinHandle<()>,
    stderr_thread: thread::JoinHandle<()>,
) {
    let was_starting = entry.snapshot().lifecycle == ManagedRuntimeLifecycle::Starting;
    entry.set_stopping();
    apply_shutdown_hook(&entry.stdin, hook);
    let deadline = Instant::now() + shutdown_timeout;
    let mut graceful_status = None;
    while Instant::now() < deadline {
        while let Ok(event) = receiver.try_recv() {
            let _ = handle_stream_event(entry, None, stdout_policy, event);
        }
        match process.try_wait() {
            Ok(Some(status)) => {
                graceful_status = Some(status);
                break;
            }
            Ok(None) => thread::sleep(MONITOR_TICK),
            Err(_) => break,
        }
    }
    let (status, graceful) = if let Some(status) = graceful_status {
        process.cleanup_descendants();
        (Some(status), true)
    } else {
        (process.force_and_wait(), false)
    };
    if let Ok(mut stdin) = entry.stdin.lock() {
        stdin.take();
    }
    let _ = drain_and_join(
        entry,
        None,
        stdout_policy,
        receiver,
        stdout_thread,
        stderr_thread,
    );
    let exit_code = status.and_then(|status| status.code());
    let (lifecycle, kind, failure) = match reason {
        StopReason::Requested => (
            ManagedRuntimeLifecycle::Exited,
            if graceful {
                ManagedRuntimeTerminationKind::GracefulStop
            } else {
                ManagedRuntimeTerminationKind::ForcedStop
            },
            None,
        ),
        StopReason::Cancelled if was_starting => (
            ManagedRuntimeLifecycle::Failed,
            ManagedRuntimeTerminationKind::StartupFailed,
            Some(ManagedRuntimeErrorCode::StartupCancelled),
        ),
        StopReason::Cancelled => (
            ManagedRuntimeLifecycle::Exited,
            ManagedRuntimeTerminationKind::Cancelled,
            None,
        ),
        StopReason::StartupTimeout => (
            ManagedRuntimeLifecycle::Failed,
            ManagedRuntimeTerminationKind::StartupFailed,
            Some(ManagedRuntimeErrorCode::StartupTimeout),
        ),
        StopReason::Deadline => (
            ManagedRuntimeLifecycle::Failed,
            ManagedRuntimeTerminationKind::DeadlineExceeded,
            Some(ManagedRuntimeErrorCode::DeadlineExceeded),
        ),
        StopReason::None => unreachable!(),
    };
    entry.finish(
        lifecycle,
        ManagedRuntimeTermination { kind, exit_code },
        failure,
    );
}

#[allow(clippy::too_many_arguments)]
fn finish_failed_process(
    process: &mut ProcessTree,
    entry: &RuntimeEntry,
    failure: ManagedRuntimeErrorCode,
    kind: ManagedRuntimeTerminationKind,
    stdout_policy: RuntimeStdoutPolicy,
    receiver: &mpsc::Receiver<StreamEvent>,
    stdout_thread: thread::JoinHandle<()>,
    stderr_thread: thread::JoinHandle<()>,
) {
    let status = process.force_and_wait();
    if let Ok(mut stdin) = entry.stdin.lock() {
        stdin.take();
    }
    let _ = drain_and_join(
        entry,
        None,
        stdout_policy,
        receiver,
        stdout_thread,
        stderr_thread,
    );
    entry.finish(
        ManagedRuntimeLifecycle::Failed,
        ManagedRuntimeTermination {
            kind,
            exit_code: status.and_then(|status| status.code()),
        },
        Some(failure),
    );
}

fn drain_and_join(
    entry: &RuntimeEntry,
    readiness: Option<&ValidatedReadiness>,
    stdout_policy: RuntimeStdoutPolicy,
    receiver: &mpsc::Receiver<StreamEvent>,
    stdout_thread: thread::JoinHandle<()>,
    stderr_thread: thread::JoinHandle<()>,
) -> Option<ManagedRuntimeErrorCode> {
    let mut failure = None;
    let deadline = Instant::now() + OUTPUT_DRAIN_TIMEOUT;
    while (!stdout_thread.is_finished() || !stderr_thread.is_finished())
        && Instant::now() < deadline
    {
        if let Ok(event) = receiver.recv_timeout(MONITOR_TICK) {
            failure =
                failure.or_else(|| handle_stream_event(entry, readiness, stdout_policy, event));
        }
    }
    while let Ok(event) = receiver.try_recv() {
        failure = failure.or_else(|| handle_stream_event(entry, readiness, stdout_policy, event));
    }
    let stdout_finished = stdout_thread.is_finished();
    let stderr_finished = stderr_thread.is_finished();
    if stdout_finished {
        let _ = stdout_thread.join();
    }
    if stderr_finished {
        let _ = stderr_thread.join();
    }
    if !stdout_finished || !stderr_finished {
        entry.push_stderr(b"[runtime output drain truncated]");
    }
    failure
}

fn apply_shutdown_hook(stdin: &Mutex<Option<ChildStdin>>, hook: &RuntimeShutdownHook) {
    let Ok(mut stdin) = stdin.lock() else {
        return;
    };
    match hook {
        RuntimeShutdownHook::CloseStdin => {
            stdin.take();
        }
        RuntimeShutdownHook::StdinLine(line) => {
            if let Some(handle) = stdin.as_mut() {
                let _ = handle.write_all(line.as_bytes());
                let _ = handle.write_all(b"\n");
                let _ = handle.flush();
            }
        }
    }
}

fn remove_monitored_entry(
    supervisor: &Weak<SupervisorInner>,
    key: &RuntimeKey,
    expected: &Arc<RuntimeEntry>,
) {
    let Some(supervisor) = supervisor.upgrade() else {
        return;
    };
    let Ok(mut entries) = supervisor.entries.lock() else {
        return;
    };
    if entries
        .get(key)
        .is_some_and(|candidate| Arc::ptr_eq(candidate, expected))
    {
        entries.remove(key);
    }
}

fn validated_startup_wait(
    entry: &RuntimeEntry,
    timeout: Duration,
) -> Result<(), ManagedRuntimeErrorCode> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(ManagedRuntimeErrorCode::InvalidLaunch)?;
    let mut state = entry
        .state
        .lock()
        .map_err(|_| ManagedRuntimeErrorCode::RuntimeNotActive)?;
    loop {
        match state.lifecycle {
            ManagedRuntimeLifecycle::Ready => return Ok(()),
            ManagedRuntimeLifecycle::Failed | ManagedRuntimeLifecycle::Exited => {
                return Err(state
                    .failure
                    .unwrap_or(ManagedRuntimeErrorCode::StartupFailed));
            }
            ManagedRuntimeLifecycle::Starting | ManagedRuntimeLifecycle::Stopping => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ManagedRuntimeErrorCode::StartupTimeout);
        }
        let (next, result) = entry
            .changed
            .wait_timeout(state, remaining)
            .map_err(|_| ManagedRuntimeErrorCode::RuntimeNotActive)?;
        state = next;
        if result.timed_out() {
            return Err(ManagedRuntimeErrorCode::StartupTimeout);
        }
    }
}

fn validate_selection(
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
) -> ManagedRuntimeResult<()> {
    profile
        .revalidate_for_launch()
        .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidSelection))?;
    let expectation = package.expectation();
    let binding = profile.binding();
    if profile.lifecycle() != RuntimeProfileLifecycle::Active
        || expectation.product() != binding.product()
        || expectation.runtime_id() != binding.runtime_id()
        || expectation.runtime_version() != binding.runtime_version()
    {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidSelection));
    }
    Ok(())
}

fn validate_executable(path: &Path) -> ManagedRuntimeResult<PathBuf> {
    if !path.is_absolute() {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidSelection));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidSelection))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidSelection));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidSelection))?;
    if canonical != path {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidSelection));
    }
    Ok(canonical)
}

fn validate_launch_spec(
    spec: ManagedRuntimeLaunchSpec,
    profile: &RuntimeProfile,
    expected_runtime_version: &str,
) -> ManagedRuntimeResult<ValidatedLaunchSpec> {
    if spec.args.len() > MAX_ARGS
        || spec
            .args
            .iter()
            .any(|arg| arg.len() > MAX_ARG_BYTES || arg.as_bytes().contains(&0))
        || spec.startup_timeout.is_zero()
        || spec.startup_timeout > MAX_STARTUP_TIMEOUT
        || spec.shutdown_timeout.is_zero()
        || spec.shutdown_timeout > MAX_SHUTDOWN_TIMEOUT
        || spec
            .runtime_deadline
            .is_some_and(|duration| duration.is_zero())
        || spec
            .runtime_deadline
            .is_some_and(|duration| Instant::now().checked_add(duration).is_none())
        || !valid_readiness_probe(&spec.readiness)
        || !valid_shutdown_hook(&spec.shutdown)
    {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
    }
    let working_directory = match spec.working_directory {
        Some(path) => validate_working_directory(&path)?,
        None => profile
            .environment_roots()
            .iter()
            .next()
            .map(|(_, path)| path.to_path_buf())
            .ok_or_else(|| runtime_error(ManagedRuntimeErrorCode::InvalidSelection))?,
    };
    let path_extensions = validate_path_extensions(&spec.path_extensions)?;
    if spec.environment.len() > MAX_ENVIRONMENT_ENTRIES {
        return Err(runtime_error(ManagedRuntimeErrorCode::EnvironmentRejected));
    }
    let profile_keys = profile
        .environment_roots()
        .iter()
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    for (key, value) in &spec.environment {
        if !valid_environment_key(key)
            || value.len() > MAX_ENVIRONMENT_VALUE_BYTES
            || value.as_bytes().contains(&0)
            || forbidden_environment_key(key)
            || profile_keys
                .iter()
                .any(|profile_key| *profile_key == key.as_str())
        {
            return Err(runtime_error(ManagedRuntimeErrorCode::EnvironmentRejected));
        }
    }
    let mut sensitive_values = profile
        .environment_roots()
        .iter()
        .flat_map(|(_, path)| {
            [
                path.to_string_lossy().into_owned(),
                child_environment_path(path).to_string_lossy().into_owned(),
            ]
        })
        .collect::<Vec<_>>();
    for path in [profile.launch_home_root(), profile.launch_temp_root()] {
        sensitive_values.push(path.to_string_lossy().into_owned());
        sensitive_values.push(child_environment_path(path).to_string_lossy().into_owned());
    }
    sensitive_values.extend(
        spec.environment
            .values()
            .filter(|value| Path::new(value.as_str()).is_absolute())
            .cloned(),
    );
    sensitive_values.extend(
        spec.args
            .iter()
            .filter(|value| Path::new(value.as_str()).is_absolute())
            .cloned(),
    );
    sensitive_values.push(working_directory.to_string_lossy().into_owned());
    sensitive_values.push(
        child_environment_path(&working_directory)
            .to_string_lossy()
            .into_owned(),
    );
    for path in &path_extensions {
        sensitive_values.push(path.to_string_lossy().into_owned());
        sensitive_values.push(child_environment_path(path).to_string_lossy().into_owned());
    }
    let readiness = prepare_readiness(spec.readiness, expected_runtime_version)?;
    if let Some(password) = readiness.opencode_password() {
        sensitive_values.push(password.to_owned());
    }
    Ok(ValidatedLaunchSpec {
        args: spec.args,
        working_directory,
        environment: spec.environment,
        path_extensions,
        readiness,
        shutdown: spec.shutdown,
        stdout_policy: spec.stdout_policy,
        startup_timeout: spec.startup_timeout,
        shutdown_timeout: spec.shutdown_timeout,
        runtime_deadline: spec.runtime_deadline,
        sensitive_values,
    })
}

fn valid_readiness_probe(probe: &RuntimeReadinessProbe) -> bool {
    match probe {
        RuntimeReadinessProbe::StdoutLineEquals(expected) => {
            !expected.is_empty()
                && expected.len() <= MAX_STDOUT_FRAME_BYTES
                && !expected
                    .chars()
                    .any(|character| matches!(character, '\r' | '\n' | '\0'))
        }
        RuntimeReadinessProbe::OpenCodeAuthenticatedHttpLoopback(address) => {
            address.ip().is_loopback() && address.port() != 0
        }
    }
}

fn prepare_readiness(
    probe: RuntimeReadinessProbe,
    expected_runtime_version: &str,
) -> ManagedRuntimeResult<ValidatedReadiness> {
    if !valid_expected_runtime_version(expected_runtime_version) {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidSelection));
    }
    match probe {
        RuntimeReadinessProbe::StdoutLineEquals(expected) => {
            Ok(ValidatedReadiness::StdoutLineEquals(expected))
        }
        RuntimeReadinessProbe::OpenCodeAuthenticatedHttpLoopback(address) => {
            let listener = TcpListener::bind(address)
                .map_err(|_| runtime_error(ManagedRuntimeErrorCode::ReadinessPortOccupied))?;
            let bound = listener
                .local_addr()
                .map_err(|_| runtime_error(ManagedRuntimeErrorCode::ReadinessPortOccupied))?;
            if bound != address {
                return Err(runtime_error(
                    ManagedRuntimeErrorCode::ReadinessPortOccupied,
                ));
            }
            drop(listener);
            Ok(ValidatedReadiness::OpenCodeAuthenticatedHttpLoopback {
                address,
                expected_version: expected_runtime_version.to_owned(),
                password: generate_opencode_password(),
            })
        }
    }
}

fn valid_expected_runtime_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.eq_ignore_ascii_case("latest")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn valid_shutdown_hook(hook: &RuntimeShutdownHook) -> bool {
    match hook {
        RuntimeShutdownHook::CloseStdin => true,
        RuntimeShutdownHook::StdinLine(line) => {
            !line.is_empty()
                && line.len() <= MAX_STDOUT_FRAME_BYTES
                && !line
                    .chars()
                    .any(|character| matches!(character, '\r' | '\n' | '\0'))
        }
    }
}

fn validate_working_directory(path: &Path) -> ManagedRuntimeResult<PathBuf> {
    if !path.is_absolute() {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidLaunch))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
    }
    path.canonicalize()
        .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidLaunch))
}

fn validate_path_extensions(paths: &[PathBuf]) -> ManagedRuntimeResult<Vec<PathBuf>> {
    if paths.len() > MAX_PATH_EXTENSION_ENTRIES {
        return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
    }
    let mut total_bytes = 0usize;
    let mut validated = Vec::with_capacity(paths.len());
    for path in paths {
        total_bytes = total_bytes
            .checked_add(path.as_os_str().to_string_lossy().len())
            .ok_or_else(|| runtime_error(ManagedRuntimeErrorCode::InvalidLaunch))?;
        if total_bytes > MAX_PATH_EXTENSION_BYTES
            || !path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
        {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidLaunch))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| runtime_error(ManagedRuntimeErrorCode::InvalidLaunch))?;
        if !same_environment_path(&canonical, path) || validated.contains(&canonical) {
            return Err(runtime_error(ManagedRuntimeErrorCode::InvalidLaunch));
        }
        validated.push(canonical);
    }
    Ok(validated)
}

#[cfg(not(windows))]
fn same_environment_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn same_environment_path(left: &Path, right: &Path) -> bool {
    child_environment_path(left) == child_environment_path(right)
}

fn valid_environment_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_ENVIRONMENT_KEY_BYTES
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && key
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'_')
}

fn forbidden_environment_key(key: &str) -> bool {
    let canonical = key
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let canonical = String::from_utf8_lossy(&canonical);
    matches!(
        canonical.as_ref(),
        "path"
            | "home"
            | "userprofile"
            | "homedrive"
            | "homepath"
            | "shell"
            | "comspec"
            | "pathext"
            | "systemroot"
            | "systemdrive"
            | "windir"
            | "numberofprocessors"
            | "processorarchitecture"
            | "appdata"
            | "localappdata"
            | "programdata"
            | "programfiles"
            | "programfilesx86"
            | "programw6432"
            | "opencodeserverusername"
            | "temp"
            | "tmp"
            | "tmpdir"
            | "authorization"
            | "cookie"
    ) || canonical.contains("password")
        || canonical.starts_with("alfredruntimehealth")
        || canonical.contains("auth")
        || canonical.contains("secret")
        || canonical.contains("token")
        || canonical.contains("credential")
        || canonical.contains("apikey")
        || canonical.contains("privatekey")
}

fn spawn_runtime(
    executable: &Path,
    profile: &RuntimeProfile,
    spec: &ValidatedLaunchSpec,
) -> Result<(ProcessTree, ChildStdin, ChildStdout, ChildStderr), ()> {
    let mut command = Command::new(executable);
    command
        .args(&spec.args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.current_dir(&spec.working_directory);
    for (key, value) in code_owned_launch_environment(executable, profile, &spec.path_extensions)? {
        command.env(key, value);
    }
    if let Some(password) = spec.readiness.opencode_password() {
        command.env(OPENCODE_SERVER_PASSWORD_ENV, password);
    }
    for (key, value) in &spec.environment {
        command.env(key, value);
    }
    for (key, path) in profile.environment_roots().iter() {
        command.env(key, child_environment_path(path));
    }
    let mut process = ProcessTree::spawn(&mut command).map_err(|_| ())?;
    let stdin = process.child.stdin.take().ok_or(())?;
    let stdout = process.child.stdout.take().ok_or(())?;
    let stderr = process.child.stderr.take().ok_or(())?;
    Ok((process, stdin, stdout, stderr))
}

#[cfg(unix)]
fn code_owned_launch_environment(
    executable: &Path,
    profile: &RuntimeProfile,
    path_extensions: &[PathBuf],
) -> Result<Vec<(OsString, OsString)>, ()> {
    let package_bin = executable.parent().ok_or(())?;
    let mut path_entries = vec![package_bin];
    #[cfg(target_os = "macos")]
    for homebrew_bin in [Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")] {
        if fs::symlink_metadata(homebrew_bin)
            .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir())
            .unwrap_or(false)
        {
            path_entries.push(homebrew_bin);
        }
    }
    path_entries.extend([
        Path::new("/usr/bin"),
        Path::new("/bin"),
        Path::new("/usr/sbin"),
        Path::new("/sbin"),
    ]);
    path_entries.extend(path_extensions.iter().map(|path| path.as_path()));
    let path = std::env::join_paths(path_entries).map_err(|_| ())?;
    Ok(vec![
        (
            OsString::from("HOME"),
            child_environment_path(profile.launch_home_root()),
        ),
        (
            OsString::from("TMPDIR"),
            child_environment_path(profile.launch_temp_root()),
        ),
        (
            OsString::from("TMP"),
            child_environment_path(profile.launch_temp_root()),
        ),
        (
            OsString::from("TEMP"),
            child_environment_path(profile.launch_temp_root()),
        ),
        (OsString::from("PATH"), path),
    ])
}

#[cfg(windows)]
fn code_owned_launch_environment(
    executable: &Path,
    profile: &RuntimeProfile,
    path_extensions: &[PathBuf],
) -> Result<Vec<(OsString, OsString)>, ()> {
    let package_bin = executable.parent().ok_or(())?;
    let system_root = windows_process::windows_directory().map_err(|_| ())?;
    if !system_root.is_absolute()
        || system_root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(());
    }
    let system_root_value = child_environment_path(&system_root);
    let system32 = system_root.join("System32");
    let mut path_entries = vec![
        package_bin.to_path_buf(),
        system32.clone(),
        system_root.clone(),
        system32.join("Wbem"),
        system32.join("WindowsPowerShell/v1.0"),
    ];
    path_entries.extend(path_extensions.iter().cloned());
    let path = std::env::join_paths(path_entries).map_err(|_| ())?;
    let home_path = profile.launch_home_root();
    let home = child_environment_path(profile.launch_home_root());
    let temp = child_environment_path(profile.launch_temp_root());
    let app_data = child_environment_path(&home_path.join("AppData/Roaming"));
    let local_app_data = child_environment_path(&home_path.join("AppData/Local"));
    let system_drive_root = system_root.parent().ok_or(())?;
    let program_data = child_environment_path(&system_drive_root.join("ProgramData"));
    let program_files = child_environment_path(&system_drive_root.join("Program Files"));
    let processor_architecture = match std::env::consts::ARCH {
        "x86" => "x86",
        "x86_64" => "AMD64",
        "aarch64" => "ARM64",
        _ => return Err(()),
    };
    let mut environment = vec![
        (OsString::from("HOME"), home.clone()),
        (OsString::from("USERPROFILE"), home.clone()),
        (OsString::from("APPDATA"), app_data),
        (OsString::from("LOCALAPPDATA"), local_app_data),
        (OsString::from("PROGRAMDATA"), program_data),
        (OsString::from("ProgramFiles"), program_files.clone()),
        (OsString::from("ProgramW6432"), program_files),
        (
            OsString::from("PROCESSOR_ARCHITECTURE"),
            OsString::from(processor_architecture),
        ),
        (OsString::from("TEMP"), temp.clone()),
        (OsString::from("TMP"), temp),
        (OsString::from("SystemRoot"), system_root_value.clone()),
        (OsString::from("windir"), system_root_value.clone()),
        (
            OsString::from("PATHEXT"),
            OsString::from(".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC"),
        ),
        (
            OsString::from("NUMBER_OF_PROCESSORS"),
            thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1)
                .to_string()
                .into(),
        ),
        (OsString::from("PATH"), path),
        (
            OsString::from("COMSPEC"),
            child_environment_path(&system32.join("cmd.exe")),
        ),
    ];
    let root_text = system_root_value.to_string_lossy();
    if root_text.len() >= 2 && root_text.as_bytes()[1] == b':' {
        environment.push((
            OsString::from("SystemDrive"),
            OsString::from(&root_text[..2]),
        ));
    }
    let home_text = home.to_string_lossy();
    if home_text.len() >= 3 && home_text.as_bytes()[1] == b':' {
        environment.push((OsString::from("HOMEDRIVE"), OsString::from(&home_text[..2])));
        environment.push((OsString::from("HOMEPATH"), OsString::from(&home_text[2..])));
    }
    Ok(environment)
}

#[cfg(not(windows))]
fn child_environment_path(path: &Path) -> OsString {
    path.as_os_str().to_owned()
}

#[cfg(windows)]
fn child_environment_path(path: &Path) -> OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let verbatim = r"\\?\".encode_utf16().collect::<Vec<_>>();
    let verbatim_unc = r"\\?\UNC\".encode_utf16().collect::<Vec<_>>();
    if encoded.starts_with(&verbatim_unc) {
        let mut normalized = r"\\".encode_utf16().collect::<Vec<_>>();
        normalized.extend_from_slice(&encoded[verbatim_unc.len()..]);
        OsString::from_wide(&normalized)
    } else if encoded.starts_with(&verbatim) {
        OsString::from_wide(&encoded[verbatim.len()..])
    } else {
        OsString::from_wide(&encoded)
    }
}

struct ProcessTree {
    child: Child,
    process_group_id: u32,
    #[cfg(windows)]
    job: Option<windows_process::JobHandle>,
    cleaned: bool,
}

impl ProcessTree {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_process::CREATE_NEW_PROCESS_GROUP);
        }
        #[cfg(windows)]
        let mut child = command.spawn()?;
        #[cfg(not(windows))]
        let child = command.spawn()?;
        let process_group_id = child.id();
        #[cfg(windows)]
        let job = match windows_process::JobHandle::assign(&child) {
            Ok(job) => Some(job),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            process_group_id,
            #[cfg(windows)]
            job,
            cleaned: false,
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        #[cfg(unix)]
        if !self.cleaned && unix_process::exited_without_reaping(self.child.id())? {
            self.terminate_tree();
            thread::sleep(PROCESS_TERM_GRACE);
            self.kill_tree();
            self.cleaned = true;
        }
        self.child.try_wait()
    }

    fn cleanup_descendants(&mut self) {
        if self.cleaned {
            return;
        }
        self.kill_tree();
        self.cleaned = true;
    }

    fn force_and_wait(&mut self) -> Option<ExitStatus> {
        if !self.cleaned {
            self.terminate_tree();
            let deadline = Instant::now() + PROCESS_TERM_GRACE;
            while Instant::now() < deadline {
                match self.try_wait() {
                    Ok(Some(status)) => return Some(status),
                    Ok(None) => thread::sleep(MONITOR_TICK),
                    Err(_) => break,
                }
            }
            self.kill_tree();
            let _ = self.child.kill();
        }
        let status = self.child.wait().ok();
        self.cleaned = true;
        status
    }

    fn kill_tree(&mut self) {
        #[cfg(unix)]
        unix_process::signal_process_group(self.process_group_id, unix_process::SIGKILL);
        #[cfg(windows)]
        {
            self.job.take();
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = self.child.kill();
        }
    }

    fn terminate_tree(&mut self) {
        #[cfg(unix)]
        unix_process::signal_process_group(self.process_group_id, unix_process::SIGTERM);
        #[cfg(not(unix))]
        {}
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.force_and_wait();
        }
    }
}

#[cfg(unix)]
mod unix_process {
    use std::io;
    use std::mem;

    pub(super) const SIGKILL: i32 = libc::SIGKILL;
    pub(super) const SIGTERM: i32 = libc::SIGTERM;

    pub(super) fn signal_process_group(process_group_id: u32, signal: i32) {
        let Ok(process_group_id) = i32::try_from(process_group_id) else {
            return;
        };
        // SAFETY: `kill` is called with a process-group id created for this
        // exact child. Failure is harmless and followed by `Child::kill`.
        let _ = unsafe { libc::kill(-process_group_id, signal) };
    }

    pub(super) fn exited_without_reaping(process_id: u32) -> io::Result<bool> {
        let process_id = libc::pid_t::try_from(process_id)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process id"))?;
        // SAFETY: the zeroed `siginfo_t` is valid output storage for waitid;
        // WNOWAIT observes the child without consuming its exit status.
        let mut information: libc::siginfo_t = unsafe { mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                process_id as libc::id_t,
                &mut information,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: waitid initialized the siginfo storage on success.
        Ok(unsafe { information.si_pid() } == process_id)
    }
}

#[cfg(windows)]
mod windows_process {
    use std::ffi::c_void;
    use std::io;
    use std::mem;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::PathBuf;
    use std::process::Child;
    use std::ptr;

    type Handle = *mut c_void;
    type Bool = i32;
    type Dword = u32;
    type LargeInteger = i64;

    pub(super) const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

    #[repr(C)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct BasicLimitInformation {
        per_process_user_time_limit: LargeInteger,
        per_job_user_time_limit: LargeInteger,
        limit_flags: Dword,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: Dword,
        affinity: usize,
        priority_class: Dword,
        scheduling_class: Dword,
    }

    #[repr(C)]
    struct ExtendedLimitInformation {
        basic_limit_information: BasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            information_class: i32,
            information: *const c_void,
            information_length: Dword,
        ) -> Bool;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> Bool;
        fn CloseHandle(handle: Handle) -> Bool;
        fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }

    pub(super) struct JobHandle(Handle);

    pub(super) fn windows_directory() -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 32_768];
        // SAFETY: the buffer is writable for the supplied length, and the API
        // returns the number of initialized UTF-16 code units.
        let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 || length as usize >= buffer.len() {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(length as usize);
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
    }

    unsafe impl Send for JobHandle {}

    impl JobHandle {
        pub(super) fn assign(child: &Child) -> io::Result<Self> {
            // SAFETY: every handle is checked before use, and the information
            // structure matches JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
            unsafe {
                let job = CreateJobObjectW(ptr::null_mut(), ptr::null());
                if job.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let mut information: ExtendedLimitInformation = mem::zeroed();
                information.basic_limit_information.limit_flags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &information as *const _ as *const c_void,
                    mem::size_of::<ExtendedLimitInformation>() as Dword,
                ) == 0
                    || AssignProcessToJobObject(job, child.as_raw_handle() as Handle) == 0
                {
                    let error = io::Error::last_os_error();
                    let _ = CloseHandle(job);
                    return Err(error);
                }
                Ok(Self(job))
            }
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // SAFETY: this type exclusively owns the non-null job handle.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

fn redact_diagnostic(value: &str, sensitive_values: &[String]) -> String {
    let mut redacted = value.to_owned();
    for sensitive in sensitive_values {
        if sensitive.len() >= 4 {
            redacted = redacted.replace(sensitive, "[REDACTED PATH]");
        }
    }
    let shared = redact_text(&redacted);
    if contains_diagnostic_secret(&redacted) || shared != redacted {
        "[REDACTED]".into()
    } else {
        shared
    }
}

fn runtime_error(code: ManagedRuntimeErrorCode) -> ManagedRuntimeError {
    ManagedRuntimeError::new(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_accounts::models::{AgentProductId, ManagedRuntimeId};
    use crate::agent_accounts::runtime_profile::{RuntimeProfileBinding, RuntimeProfileStore};
    use crate::agents::runtime_package::{
        PublisherVerificationScheme, RuntimeArtifactManifest, RuntimeLicenseNoticeRequirements,
        RuntimePackageExpectation, RuntimePackageManifest, RuntimePackageStore,
        RuntimePackageVerification, RuntimePublisherRequirement, RuntimeRollbackMetadata,
        RuntimeTargetManifest, RuntimeUpdatePolicy, RUNTIME_PACKAGE_CONTRACT_VERSION,
        RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
    };
    use crate::agents::OpaqueAgentAccountRef;
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader};
    use std::sync::OnceLock;

    const FIXTURE_ENV: &str = "ALFRED_MANAGED_RUNTIME_FIXTURE";
    const FIXTURE_FILE_ENV: &str = "ALFRED_MANAGED_RUNTIME_FIXTURE_FILE";
    const FIXTURE_ADDRESS_ENV: &str = "ALFRED_MANAGED_RUNTIME_FIXTURE_ADDRESS";
    const FIXTURE_TEST: &str = "agents::managed_runtime::tests::managed_runtime_fixture_child";

    struct Fixture {
        selection: RuntimePackageSelection,
        profile_store: RuntimeProfileStore,
        app_data: PathBuf,
    }

    fn fixture() -> &'static Fixture {
        static FIXTURE: OnceLock<Fixture> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            let app_data = std::env::temp_dir().join(format!(
                "alfred-managed-runtime-supervisor-{}",
                uuid::Uuid::new_v4().simple()
            ));
            let source = app_data.join("source");
            fs::create_dir_all(source.join("bin")).unwrap();
            fs::create_dir_all(source.join("legal")).unwrap();
            let executable = source.join(if cfg!(windows) {
                "bin/runtime.exe"
            } else {
                "bin/runtime"
            });
            fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            }
            fs::write(source.join("legal/LICENSE.txt"), b"fixture license").unwrap();
            fs::write(source.join("legal/NOTICE.txt"), b"fixture notice").unwrap();
            let digest = |path: &Path| {
                let bytes = fs::read(path).unwrap();
                format!("{:x}", Sha256::digest(bytes))
            };
            let executable_relative = if cfg!(windows) {
                "bin/runtime.exe"
            } else {
                "bin/runtime"
            };
            let target = "fixture-target";
            let version = "0.147.0";
            let manifest = RuntimePackageManifest {
                schema_version: RUNTIME_PACKAGE_MANIFEST_SCHEMA_VERSION,
                contract_version: RUNTIME_PACKAGE_CONTRACT_VERSION,
                runtime_id: ManagedRuntimeId::CodexPythonSdk,
                runtime_version: version.into(),
                update_policy: RuntimeUpdatePolicy {
                    alfred_managed: true,
                    self_update_allowed: false,
                    path_lookup_allowed: false,
                },
                targets: vec![RuntimeTargetManifest {
                    target: target.into(),
                    executable: RuntimeArtifactManifest {
                        relative_path: executable_relative.into(),
                        sha256: digest(&executable),
                    },
                    resources: vec![
                        RuntimeArtifactManifest {
                            relative_path: "legal/LICENSE.txt".into(),
                            sha256: digest(&source.join("legal/LICENSE.txt")),
                        },
                        RuntimeArtifactManifest {
                            relative_path: "legal/NOTICE.txt".into(),
                            sha256: digest(&source.join("legal/NOTICE.txt")),
                        },
                    ],
                    publisher_verification: RuntimePublisherRequirement {
                        scheme: PublisherVerificationScheme::PlatformPackageSignature,
                        publisher: "fixture-publisher".into(),
                        required: true,
                    },
                    license_notice: RuntimeLicenseNoticeRequirements {
                        license_expression: "Apache-2.0".into(),
                        license_resource_path: "legal/LICENSE.txt".into(),
                        notice_resource_path: "legal/NOTICE.txt".into(),
                    },
                    rollback: RuntimeRollbackMetadata {
                        retain_previous_verified: true,
                        automatic_fallback: false,
                    },
                }],
            };
            let expectation =
                RuntimePackageExpectation::for_product(AgentProductId::ChatgptCodex, target)
                    .unwrap();
            let verification =
                RuntimePackageVerification::verified_fixture(manifest.clone(), expectation)
                    .unwrap();
            let package_store = RuntimePackageStore::open(&app_data).unwrap();
            package_store
                .stage_and_activate(&source, &verification, None)
                .unwrap();
            let selection = package_store.select_active(&verification).unwrap();
            let profile_store = RuntimeProfileStore::new(&app_data).unwrap();
            Fixture {
                selection,
                profile_store,
                app_data,
            }
        })
    }

    fn profile() -> RuntimeProfile {
        let fixture = fixture();
        let account = OpaqueAgentAccountRef::parse(&format!(
            "account_fixture_{}",
            uuid::Uuid::new_v4().simple()
        ))
        .unwrap();
        let binding = RuntimeProfileBinding::new(
            &account,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        )
        .unwrap();
        fixture.profile_store.create(&binding).unwrap()
    }

    fn launch_spec(mode: &str) -> ManagedRuntimeLaunchSpec {
        let environment = BTreeMap::from([(FIXTURE_ENV.into(), mode.into())]);
        ManagedRuntimeLaunchSpec::new(
            vec!["--exact".into(), FIXTURE_TEST.into(), "--nocapture".into()],
            RuntimeReadinessProbe::stdout_line_equals("READY").unwrap(),
            RuntimeShutdownHook::stdin_line("shutdown").unwrap(),
            RuntimeStdoutPolicy::TypedFramesFailClosed,
        )
        .with_environment(environment)
        .with_startup_timeout(Duration::from_secs(2))
        .with_shutdown_timeout(Duration::from_millis(120))
    }

    fn http_launch_spec(mode: &str, address: SocketAddr) -> ManagedRuntimeLaunchSpec {
        let environment = BTreeMap::from([
            (FIXTURE_ENV.into(), mode.into()),
            (FIXTURE_ADDRESS_ENV.into(), address.to_string()),
        ]);
        ManagedRuntimeLaunchSpec::new(
            vec!["--exact".into(), FIXTURE_TEST.into(), "--nocapture".into()],
            RuntimeReadinessProbe::opencode_authenticated_http_loopback(address).unwrap(),
            RuntimeShutdownHook::stdin_line("shutdown").unwrap(),
            RuntimeStdoutPolicy::LogsDropOldest,
        )
        .with_environment(environment)
        .with_startup_timeout(Duration::from_secs(2))
        .with_shutdown_timeout(Duration::from_millis(120))
    }

    #[test]
    fn launches_verified_active_package_with_exact_profile_and_stops_gracefully() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let profile = profile();
        let handle = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("success"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        assert_eq!(handle.snapshot().lifecycle, ManagedRuntimeLifecycle::Ready);
        assert!(!format!("{handle:?}").contains(fixture().app_data.to_string_lossy().as_ref()));
        assert!(!format!("{handle:?}").contains(profile.profile_ref().as_str()));
        let stopped = handle.stop().unwrap();
        assert_eq!(stopped.lifecycle, ManagedRuntimeLifecycle::Exited);
        assert_eq!(
            stopped.termination.unwrap().kind,
            ManagedRuntimeTerminationKind::GracefulStop
        );
    }

    #[test]
    fn rejects_wrong_package_profile_binding_before_spawn() {
        let account = OpaqueAgentAccountRef::parse("account_wrong_runtime_0001").unwrap();
        let binding = RuntimeProfileBinding::new(
            &account,
            AgentProductId::ClaudeCodeSubscription,
            ManagedRuntimeId::ClaudeCodeManaged,
            "2.1.246",
        )
        .unwrap();
        let wrong = fixture().profile_store.create(&binding).unwrap();
        let error = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &wrong,
                launch_spec("success"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap_err();
        assert_eq!(error.code(), ManagedRuntimeErrorCode::InvalidSelection);
        assert_eq!(format!("{error:?}"), error.code().as_str());
        assert!(!format!("{error:?}").contains(fixture().app_data.to_string_lossy().as_ref()));

        let account = OpaqueAgentAccountRef::parse("account_preserved_runtime_0001").unwrap();
        let binding = RuntimeProfileBinding::new(
            &account,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        )
        .unwrap();
        let active = fixture().profile_store.create(&binding).unwrap();
        let preserved = fixture()
            .profile_store
            .preserve(active.profile_ref(), &binding)
            .unwrap();
        for stale_or_preserved in [&active, &preserved] {
            assert_eq!(
                ManagedRuntimeSupervisor::new()
                    .launch(
                        &fixture().selection,
                        stale_or_preserved,
                        launch_spec("success"),
                        ManagedRuntimeCancellation::new(),
                    )
                    .unwrap_err()
                    .code(),
                ManagedRuntimeErrorCode::InvalidSelection
            );
        }
    }

    #[test]
    fn rejects_invalid_args_and_working_directories() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let profile = profile();
        let invalid_arg = ManagedRuntimeLaunchSpec::new(
            vec!["argument\0suffix".into()],
            RuntimeReadinessProbe::stdout_line_equals("READY").unwrap(),
            RuntimeShutdownHook::CloseStdin,
            RuntimeStdoutPolicy::TypedFramesFailClosed,
        );
        assert_eq!(
            supervisor
                .launch(
                    &fixture().selection,
                    &profile,
                    invalid_arg,
                    ManagedRuntimeCancellation::new(),
                )
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::InvalidLaunch
        );
        assert_eq!(
            supervisor
                .launch(
                    &fixture().selection,
                    &profile,
                    launch_spec("success").with_working_directory("relative/path"),
                    ManagedRuntimeCancellation::new(),
                )
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::InvalidLaunch
        );
    }

    #[test]
    fn installs_code_owned_environment_and_rejects_baseline_and_secret_overrides() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let profile = profile();
        let handle = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("environment"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let mut frames = Vec::new();
        for _ in 0..16 {
            if let Some(frame) = handle
                .read_stdout_frame(Duration::from_millis(200))
                .unwrap()
            {
                frames.push(String::from_utf8_lossy(&frame).into_owned());
            }
        }
        assert!(frames.iter().any(|line| line == "PATH_PACKAGE=true"));
        assert!(frames.iter().any(|line| line == "HOME_PROFILE=true"));
        assert!(frames.iter().any(|line| line == "TEMP_PROFILE=true"));
        assert!(frames.iter().any(|line| line == "CWD_PROFILE=true"));
        #[cfg(windows)]
        for key in [
            "SYSTEMROOT=true",
            "SYSTEMDRIVE=true",
            "WINDIR=true",
            "PATHEXT=true",
            "NUMBER_OF_PROCESSORS=true",
            "PROCESSOR_ARCHITECTURE=true",
            "APPDATA=true",
            "LOCALAPPDATA=true",
            "PROGRAMDATA=true",
            "ProgramFiles=true",
        ] {
            assert!(frames.iter().any(|line| line == key), "missing {key}");
        }
        handle.stop().unwrap();

        for key in [
            "PATH",
            "HOME",
            "USERPROFILE",
            "TEMP",
            "TMP",
            "TMPDIR",
            "SystemRoot",
            "SystemDrive",
            "windir",
            "PATHEXT",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
            "APPDATA",
            "LOCALAPPDATA",
            "PROGRAMDATA",
            "PROGRAMFILES",
            "OPENCODE_SERVER_USERNAME",
            "OPENCODE_SERVER_PASSWORD",
            "CODEX_HOME",
            "OPENAI_API_TOKEN",
        ] {
            let mut spec = launch_spec("success");
            spec.environment
                .insert(key.into(), "should-not-launch".into());
            assert_eq!(
                supervisor
                    .launch(
                        &fixture().selection,
                        &profile,
                        spec,
                        ManagedRuntimeCancellation::new(),
                    )
                    .unwrap_err()
                    .code(),
                ManagedRuntimeErrorCode::EnvironmentRejected
            );
        }
    }

    #[test]
    fn path_extensions_append_after_code_owned_entries_and_reject_unsafe_paths() {
        let extension_root = fixture()
            .app_data
            .join(format!("path-extensions-{}", uuid::Uuid::new_v4().simple()));
        let first = extension_root.join("first");
        let second = extension_root.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();

        let spec = launch_spec("success")
            .with_path_extensions(vec![first.clone(), second.clone()])
            .unwrap();
        let profile = profile();
        let executable = fixture()
            .selection
            .verified_active_executable_path()
            .unwrap();
        let environment =
            code_owned_launch_environment(&executable, &profile, &[first.clone(), second.clone()])
                .unwrap();
        let path = environment
            .iter()
            .find(|(key, _)| key.as_os_str() == std::ffi::OsStr::new("PATH"))
            .map(|(_, value)| value)
            .unwrap();
        let entries = std::env::split_paths(path).collect::<Vec<_>>();
        assert_eq!(entries.first().unwrap(), executable.parent().unwrap());
        assert_eq!(entries.get(entries.len() - 2), Some(&first));
        assert_eq!(entries.last(), Some(&second));

        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile,
                spec,
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        handle.stop().unwrap();

        for rejected in [
            PathBuf::from("relative/bin"),
            extension_root.join("missing"),
            first.join("..").join("first"),
            PathBuf::from(format!("/{}", "x".repeat(MAX_PATH_EXTENSION_BYTES + 1))),
        ] {
            assert_eq!(
                launch_spec("success")
                    .with_path_extensions(vec![rejected])
                    .unwrap_err()
                    .code(),
                ManagedRuntimeErrorCode::InvalidLaunch
            );
        }
        assert_eq!(
            launch_spec("success")
                .with_path_extensions(vec![first.clone(); MAX_PATH_EXTENSION_ENTRIES + 1])
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::InvalidLaunch
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let linked = extension_root.join("linked");
            symlink(&first, &linked).unwrap();
            assert_eq!(
                launch_spec("success")
                    .with_path_extensions(vec![linked])
                    .unwrap_err()
                    .code(),
                ManagedRuntimeErrorCode::InvalidLaunch
            );
        }
        fs::remove_dir_all(extension_root).unwrap();
    }

    #[test]
    fn authenticated_http_readiness_rejects_occupied_ports_and_accepts_keep_alive() {
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = occupied.local_addr().unwrap();
        assert_eq!(
            ManagedRuntimeSupervisor::new()
                .launch(
                    &fixture().selection,
                    &profile(),
                    http_launch_spec("http_health_keep_alive", address),
                    ManagedRuntimeCancellation::new(),
                )
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::ReadinessPortOccupied
        );
        drop(occupied);

        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                http_launch_spec("http_health_keep_alive", address),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        assert_eq!(handle.snapshot().lifecycle, ManagedRuntimeLifecycle::Ready);
        handle.stop().unwrap();
    }

    #[test]
    fn authenticated_http_readiness_rejects_an_unrelated_listener_response() {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        assert_eq!(
            ManagedRuntimeSupervisor::new()
                .launch(
                    &fixture().selection,
                    &profile(),
                    http_launch_spec("http_health_unrelated", address),
                    ManagedRuntimeCancellation::new(),
                )
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::ReadinessHandshakeFailed
        );
    }

    #[test]
    fn log_output_drops_oldest_and_oversized_stderr_is_truncated() {
        let mut logs = launch_spec("log_flood");
        logs.stdout_policy = RuntimeStdoutPolicy::LogsDropOldest;
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                logs,
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(handle.snapshot().lifecycle, ManagedRuntimeLifecycle::Ready);
        handle.stop().unwrap();

        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("oversized_stderr"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(handle.snapshot().lifecycle, ManagedRuntimeLifecycle::Ready);
        assert!(handle
            .redacted_stderr_tail()
            .contains("[stderr line truncated]"));
        handle.stop().unwrap();
    }

    #[test]
    fn legacy_ready_nonce_lines_are_never_exposed_after_startup() {
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("legacy_ready_line"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let mut frames = Vec::new();
        for _ in 0..4 {
            if let Some(frame) = handle
                .read_stdout_frame(Duration::from_millis(100))
                .unwrap()
            {
                frames.push(frame);
            }
        }
        assert!(frames
            .iter()
            .all(|frame| !frame.starts_with(LEGACY_READY_PREFIX)));
        assert!(frames
            .iter()
            .any(|frame| frame.as_slice() == b"after-control"));
        handle.stop().unwrap();
    }

    #[test]
    fn oversized_stdout_fails_and_is_reaped() {
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("oversized"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let snapshot = handle.wait_for_terminal(Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.lifecycle, ManagedRuntimeLifecycle::Failed);
        assert_eq!(
            snapshot.failure,
            Some(ManagedRuntimeErrorCode::OutputLimitExceeded)
        );
    }

    #[test]
    fn startup_timeout_kills_the_process_tree_without_an_orphan() {
        let survival = fixture().app_data.join(format!(
            "startup-survival-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let mut spec = launch_spec("startup_tree").with_startup_timeout(Duration::from_millis(80));
        spec.environment.insert(
            FIXTURE_FILE_ENV.into(),
            survival.to_string_lossy().into_owned(),
        );
        let error = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                spec,
                ManagedRuntimeCancellation::new(),
            )
            .unwrap_err();
        assert_eq!(error.code(), ManagedRuntimeErrorCode::StartupTimeout);
        thread::sleep(Duration::from_millis(450));
        assert!(!survival.exists());
    }

    #[test]
    fn duplicate_profile_runtime_launch_is_refused() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let profile = profile();
        let first = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("success"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        assert_eq!(
            supervisor
                .launch(
                    &fixture().selection,
                    &profile,
                    launch_spec("success"),
                    ManagedRuntimeCancellation::new(),
                )
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::DuplicateLaunch
        );
        first.stop().unwrap();
    }

    #[test]
    fn profile_purge_is_blocked_until_the_supervisor_handle_is_dropped() {
        let fixture = fixture();
        let account = OpaqueAgentAccountRef::parse("account_profile_lease_0001").unwrap();
        let binding = RuntimeProfileBinding::new(
            &account,
            AgentProductId::ChatgptCodex,
            ManagedRuntimeId::CodexPythonSdk,
            "0.147.0",
        )
        .unwrap();
        let profile = fixture.profile_store.create(&binding).unwrap();
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture.selection,
                &profile,
                launch_spec("success"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        assert_eq!(
            fixture
                .profile_store
                .purge(profile.profile_ref(), &binding)
                .unwrap_err()
                .code(),
            crate::agent_accounts::runtime_profile::RuntimeProfileErrorCode::ProfileInUse
        );
        handle.stop().unwrap();
        assert!(fixture
            .profile_store
            .purge(profile.profile_ref(), &binding)
            .is_err());
        drop(handle);
        fixture
            .profile_store
            .purge(profile.profile_ref(), &binding)
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn detached_pipe_holder_cannot_wedge_terminal_cleanup_or_restart() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let profile = profile();
        let handle = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("detached_pipe"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let snapshot = handle
            .wait_for_terminal(Duration::from_millis(900))
            .unwrap();
        assert!(snapshot.lifecycle.terminal());
        assert!(handle
            .redacted_stderr_tail()
            .contains("[runtime output drain truncated]"));
        let restarted = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("success"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        restarted.stop().unwrap();
    }

    #[test]
    fn forced_stop_is_reported_when_graceful_hook_is_ignored() {
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("forced"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let snapshot = handle.stop().unwrap();
        assert_eq!(snapshot.lifecycle, ManagedRuntimeLifecycle::Exited);
        assert_eq!(
            snapshot.termination.unwrap().kind,
            ManagedRuntimeTerminationKind::ForcedStop
        );
    }

    #[test]
    fn crash_is_failed_and_stderr_is_bounded_and_redacted() {
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("crash"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let snapshot = handle.wait_for_terminal(Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.lifecycle, ManagedRuntimeLifecycle::Failed);
        assert_eq!(
            snapshot.failure,
            Some(ManagedRuntimeErrorCode::RuntimeCrashed)
        );
        assert_eq!(snapshot.termination.unwrap().exit_code, Some(17));
        assert_eq!(handle.redacted_stderr_tail(), "[REDACTED]");
    }

    #[test]
    fn public_stderr_redacts_bare_jwts_and_high_entropy_tokens() {
        for secret in [
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
            "Ab3dEf5hIj7lMn9pQr2tUv4xYz6_Bc8D",
        ] {
            assert_eq!(
                redact_diagnostic(&format!("refresh failed: {secret}"), &[]),
                "[REDACTED]"
            );
        }
    }

    #[test]
    fn drop_cleans_up_the_entire_process_tree() {
        let survival = fixture()
            .app_data
            .join(format!("drop-survival-{}", uuid::Uuid::new_v4().simple()));
        let mut spec = launch_spec("ready_tree");
        spec.environment.insert(
            FIXTURE_FILE_ENV.into(),
            survival.to_string_lossy().into_owned(),
        );
        let handle = ManagedRuntimeSupervisor::new()
            .launch(
                &fixture().selection,
                &profile(),
                spec,
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        drop(handle);
        thread::sleep(Duration::from_millis(450));
        assert!(!survival.exists());
    }

    #[test]
    fn cancellation_and_deadline_stop_live_processes() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let already_cancelled = ManagedRuntimeCancellation::new();
        already_cancelled.cancel();
        assert_eq!(
            supervisor
                .launch(
                    &fixture().selection,
                    &profile(),
                    launch_spec("success"),
                    already_cancelled,
                )
                .unwrap_err()
                .code(),
            ManagedRuntimeErrorCode::StartupCancelled
        );

        let cancellation = ManagedRuntimeCancellation::new();
        let cancelled = supervisor
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("forced"),
                cancellation.clone(),
            )
            .unwrap();
        cancellation.cancel();
        assert_eq!(
            cancelled
                .wait_for_terminal(Duration::from_secs(2))
                .unwrap()
                .termination
                .unwrap()
                .kind,
            ManagedRuntimeTerminationKind::Cancelled
        );

        let deadline = supervisor
            .launch(
                &fixture().selection,
                &profile(),
                launch_spec("forced").with_runtime_deadline(Duration::from_millis(80)),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        let snapshot = deadline.wait_for_terminal(Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.lifecycle, ManagedRuntimeLifecycle::Failed);
        assert_eq!(
            snapshot.failure,
            Some(ManagedRuntimeErrorCode::DeadlineExceeded)
        );
    }

    #[test]
    fn allows_restart_after_observed_exit() {
        let supervisor = ManagedRuntimeSupervisor::new();
        let profile = profile();
        let first = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("clean_exit"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        assert_eq!(
            first
                .wait_for_terminal(Duration::from_secs(2))
                .unwrap()
                .lifecycle,
            ManagedRuntimeLifecycle::Exited
        );
        let second = supervisor
            .launch(
                &fixture().selection,
                &profile,
                launch_spec("success"),
                ManagedRuntimeCancellation::new(),
            )
            .unwrap();
        second.stop().unwrap();
    }

    #[test]
    fn managed_runtime_fixture_child() {
        let Ok(mode) = std::env::var(FIXTURE_ENV) else {
            return;
        };
        match mode.as_str() {
            "success" => {
                println!("READY");
                let mut lines = BufReader::new(std::io::stdin()).lines();
                if lines.next().transpose().unwrap().as_deref() == Some("shutdown") {
                    return;
                }
            }
            "environment" => {
                println!("READY");
                let current_executable = std::env::current_exe().unwrap();
                let package_bin = current_executable.parent().unwrap();
                println!(
                    "PATH_PACKAGE={}",
                    std::env::var_os("PATH")
                        .map(|value| std::env::split_paths(&value).any(|path| path == package_bin))
                        .unwrap_or(false)
                );
                let profile_base = std::env::var_os("CODEX_HOME")
                    .map(PathBuf::from)
                    .and_then(|path| path.parent().map(Path::to_path_buf));
                println!(
                    "HOME_PROFILE={}",
                    profile_base.as_ref().map(|root| root.join("home"))
                        == std::env::var_os("HOME").map(PathBuf::from)
                );
                println!(
                    "TEMP_PROFILE={}",
                    profile_base.as_ref().map(|root| root.join("tmp"))
                        == std::env::var_os(if cfg!(windows) { "TEMP" } else { "TMPDIR" })
                            .map(PathBuf::from)
                );
                println!(
                    "CWD_PROFILE={}",
                    std::env::current_dir().ok()
                        == std::env::var_os("CODEX_HOME").map(PathBuf::from)
                );
                #[cfg(windows)]
                for key in [
                    "SYSTEMROOT",
                    "SYSTEMDRIVE",
                    "WINDIR",
                    "PATHEXT",
                    "NUMBER_OF_PROCESSORS",
                    "PROCESSOR_ARCHITECTURE",
                    "APPDATA",
                    "LOCALAPPDATA",
                    "PROGRAMDATA",
                    "ProgramFiles",
                ] {
                    println!("{key}={}", std::env::var_os(key).is_some());
                }
                let _ = BufReader::new(std::io::stdin()).read_line(&mut String::new());
            }
            "oversized" => {
                println!("READY");
                print!("{}", "x".repeat(MAX_STDOUT_FRAME_BYTES + 1));
                std::io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(30));
            }
            "oversized_stderr" => {
                println!("READY");
                eprintln!("{}", "x".repeat(MAX_STDERR_LINE_BYTES + 1));
                let _ = BufReader::new(std::io::stdin()).read_line(&mut String::new());
            }
            "log_flood" => {
                println!("READY");
                for index in 0..(MAX_BUFFERED_STDOUT_FRAMES + 64) {
                    println!("log {index}");
                }
                let _ = BufReader::new(std::io::stdin()).read_line(&mut String::new());
            }
            "http_health_keep_alive" | "http_health_unrelated" => {
                let address: SocketAddr =
                    std::env::var(FIXTURE_ADDRESS_ENV).unwrap().parse().unwrap();
                let listener = TcpListener::bind(address).unwrap();
                let (mut connection, _) = listener.accept().unwrap();
                connection
                    .set_read_timeout(Some(HEALTH_IO_TIMEOUT))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 512];
                while find_bytes(&request, b"\r\n\r\n").is_none() {
                    let read = connection.read(&mut buffer).unwrap();
                    if read == 0 || request.len().saturating_add(read) > 4096 {
                        std::process::exit(21);
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                if mode == "http_health_unrelated" {
                    connection
                        .write_all(
                            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
                        )
                        .unwrap();
                    connection.flush().unwrap();
                    thread::sleep(Duration::from_secs(30));
                }
                let password = std::env::var(OPENCODE_SERVER_PASSWORD_ENV).unwrap();
                let credentials =
                    BASE64_STANDARD.encode(format!("{OPENCODE_SERVER_USERNAME}:{password}"));
                let request = String::from_utf8(request).unwrap();
                if !request.starts_with("GET /global/health HTTP/1.1\r\n")
                    || !request.contains(&format!("\r\nAuthorization: Basic {credentials}\r\n"))
                {
                    std::process::exit(22);
                }
                let body = br#"{"healthy":true,"version":"0.147.0"}"#;
                write!(
                    connection,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                    body.len()
                )
                .unwrap();
                connection.write_all(body).unwrap();
                connection.flush().unwrap();
                let _ = BufReader::new(std::io::stdin()).read_line(&mut String::new());
            }
            "legacy_ready_line" => {
                println!("READY");
                println!("ALFRED_RUNTIME_READY 127.0.0.1:1 leaked-nonce");
                println!("after-control");
                let _ = BufReader::new(std::io::stdin()).read_line(&mut String::new());
            }
            #[cfg(unix)]
            "detached_pipe" => {
                use std::os::unix::process::CommandExt;
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", FIXTURE_TEST, "--nocapture"])
                    .env_clear()
                    .env(FIXTURE_ENV, "pipe_holder")
                    .stdin(Stdio::null())
                    .process_group(0)
                    .spawn()
                    .unwrap();
                println!("READY");
                std::io::stdout().flush().unwrap();
                thread::sleep(Duration::from_millis(60));
            }
            "pipe_holder" => {
                thread::sleep(Duration::from_millis(1200));
            }
            "startup_tree" => {
                spawn_survival_child();
                thread::sleep(Duration::from_secs(30));
            }
            "ready_tree" => {
                spawn_survival_child();
                println!("READY");
                let _ = BufReader::new(std::io::stdin()).read_line(&mut String::new());
            }
            "grandchild" => {
                thread::sleep(Duration::from_millis(300));
                fs::write(std::env::var(FIXTURE_FILE_ENV).unwrap(), b"orphaned").unwrap();
            }
            "forced" => {
                println!("READY");
                thread::sleep(Duration::from_secs(30));
            }
            "crash" => {
                println!("READY");
                std::io::stdout().flush().unwrap();
                eprintln!("authorization: Bearer fixture-secret");
                std::process::exit(17);
            }
            "clean_exit" => {
                println!("READY");
            }
            _ => std::process::exit(19),
        }
    }

    fn spawn_survival_child() {
        let file = std::env::var(FIXTURE_FILE_ENV).unwrap();
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", FIXTURE_TEST, "--nocapture"])
            .env_clear()
            .env(FIXTURE_ENV, "grandchild")
            .env(FIXTURE_FILE_ENV, file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
    }
}
