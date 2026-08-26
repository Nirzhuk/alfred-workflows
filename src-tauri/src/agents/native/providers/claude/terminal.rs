//! Cross-platform PTY transport for the exact managed Claude Code binary.
//!
//! Terminal bytes are transient, bounded, and never parsed for OAuth URLs,
//! authorization codes, tokens, or provider prompts. A future Tauri command
//! layer can relay these backend-owned session operations without teaching
//! Alfred any part of Anthropic's login protocol.

use super::package::{artifact_for_target, CLAUDE_CODE_RUNTIME_VERSION};
use crate::agent_accounts::models::AgentProductId;
use crate::agent_accounts::runtime_profile::{
    RuntimeEnvironmentVariable, RuntimeProfile, RuntimeProfileLifecycle,
    RuntimeProfileSupervisorLease,
};
use crate::agents::runtime_package::RuntimePackageSelection;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, PtySystem};
use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_TERMINAL_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_TERMINAL_OUTPUT_CHUNK_BYTES: usize = 32 * 1024;
pub const MAX_BUFFERED_TERMINAL_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_BUFFERED_TERMINAL_OUTPUT_CHUNKS: usize = 256;
pub const MAX_TERMINAL_PATH_EXTENSIONS: usize = 24;
pub const MAX_TERMINAL_READ_WAIT: Duration = Duration::from_secs(30);
pub const MAX_TERMINAL_SESSION_WAIT: Duration = Duration::from_secs(30);
const MONITOR_TICK: Duration = Duration::from_millis(20);
const TERMINATION_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeTerminalMode {
    /// Run `claude` and let the unmodified binary present its complete first
    /// run and authentication choice UI.
    Onboarding,
    /// Run the documented provider-owned browser login command.
    AuthLogin,
    /// Run the documented logout command and relay its terminal UI verbatim.
    AuthLogout,
    /// Run the ordinary interactive Claude Code terminal product. This is not
    /// `claude -p`, the Agent SDK, or an Alfred custom renderer.
    Interactive,
    #[cfg(test)]
    Fixture,
}

impl ClaudeTerminalMode {
    pub(super) fn arguments(self) -> Vec<String> {
        match self {
            Self::Onboarding | Self::Interactive => Vec::new(),
            Self::AuthLogin => vec!["auth".into(), "login".into()],
            Self::AuthLogout => vec!["auth".into(), "logout".into()],
            #[cfg(test)]
            Self::Fixture => vec![
                "--exact".into(),
                "agents::native::providers::claude::subscription_tests::claude_managed_fixture_child"
                    .into(),
                "--nocapture".into(),
            ],
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ClaudeTerminalSessionId(String);

impl ClaudeTerminalSessionId {
    fn generate() -> Self {
        Self(format!("claude_terminal_{}", uuid::Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClaudeTerminalSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClaudeTerminalSessionId")
    }
}

#[derive(Clone)]
pub struct ClaudeTerminalLaunchSpec {
    pub mode: ClaudeTerminalMode,
    pub working_directory: PathBuf,
    pub columns: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
    /// Canonical directories appended after package and operating-system
    /// paths. There is no caller-provided raw PATH or environment map.
    pub path_extensions: Vec<PathBuf>,
}

impl fmt::Debug for ClaudeTerminalLaunchSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudeTerminalLaunchSpec")
            .field("mode", &self.mode)
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .field("pixel_width", &self.pixel_width)
            .field("pixel_height", &self.pixel_height)
            .field("path_extension_count", &self.path_extensions.len())
            .finish()
    }
}

impl ClaudeTerminalLaunchSpec {
    pub fn new(
        mode: ClaudeTerminalMode,
        working_directory: impl Into<PathBuf>,
        columns: u16,
        rows: u16,
    ) -> Self {
        Self {
            mode,
            working_directory: working_directory.into(),
            columns,
            rows,
            pixel_width: 0,
            pixel_height: 0,
            path_extensions: Vec::new(),
        }
    }

    pub fn with_pixel_size(mut self, width: u16, height: u16) -> Self {
        self.pixel_width = width;
        self.pixel_height = height;
        self
    }

    pub fn with_path_extensions(mut self, paths: Vec<PathBuf>) -> Self {
        self.path_extensions = paths;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeTerminalLifecycle {
    Running,
    Exited,
    Crashed,
    Cancelled,
    OutputLimitExceeded,
    IoFailed,
}

impl ClaudeTerminalLifecycle {
    fn terminal(self) -> bool {
        self != Self::Running
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeTerminalSnapshot {
    pub session_id: ClaudeTerminalSessionId,
    pub lifecycle: ClaudeTerminalLifecycle,
    pub exit_code: Option<u32>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeTerminalOutput {
    pub session_id: ClaudeTerminalSessionId,
    pub sequence: u64,
    pub data_base64: String,
}

impl fmt::Debug for ClaudeTerminalOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudeTerminalOutput")
            .field("session_id", &self.session_id)
            .field("sequence", &self.sequence)
            .field("data_base64", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeTerminalErrorCode {
    InvalidSelection,
    InvalidProfile,
    InvalidLaunch,
    SpawnFailed,
    ProcessTreeUnavailable,
    NotActive,
    InputLimitExceeded,
    OutputLimitExceeded,
    IoFailed,
    WaitTimedOut,
}

impl ClaudeTerminalErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidSelection => "claude_terminal_selection_invalid",
            Self::InvalidProfile => "claude_terminal_profile_invalid",
            Self::InvalidLaunch => "claude_terminal_launch_invalid",
            Self::SpawnFailed => "claude_terminal_spawn_failed",
            Self::ProcessTreeUnavailable => "claude_terminal_process_tree_unavailable",
            Self::NotActive => "claude_terminal_not_active",
            Self::InputLimitExceeded => "claude_terminal_input_limit_exceeded",
            Self::OutputLimitExceeded => "claude_terminal_output_limit_exceeded",
            Self::IoFailed => "claude_terminal_io_failed",
            Self::WaitTimedOut => "claude_terminal_wait_timed_out",
        }
    }
}

pub struct ClaudeTerminalError(ClaudeTerminalErrorCode);

impl ClaudeTerminalError {
    pub fn code(&self) -> ClaudeTerminalErrorCode {
        self.0
    }
}

impl fmt::Debug for ClaudeTerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl fmt::Display for ClaudeTerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl std::error::Error for ClaudeTerminalError {}

fn terminal_error(code: ClaudeTerminalErrorCode) -> ClaudeTerminalError {
    ClaudeTerminalError(code)
}

struct SessionState {
    lifecycle: ClaudeTerminalLifecycle,
    exit_code: Option<u32>,
}

struct BufferedOutput {
    chunks: VecDeque<(u64, Vec<u8>)>,
    bytes: usize,
    next_sequence: u64,
}

struct SessionSignals {
    state: Mutex<SessionState>,
    state_changed: Condvar,
    output: Mutex<BufferedOutput>,
    output_changed: Condvar,
    output_closed: AtomicBool,
    cancellation: AtomicBool,
}

impl SessionSignals {
    fn new() -> Self {
        Self {
            state: Mutex::new(SessionState {
                lifecycle: ClaudeTerminalLifecycle::Running,
                exit_code: None,
            }),
            state_changed: Condvar::new(),
            output: Mutex::new(BufferedOutput {
                chunks: VecDeque::new(),
                bytes: 0,
                next_sequence: 1,
            }),
            output_changed: Condvar::new(),
            output_closed: AtomicBool::new(false),
            cancellation: AtomicBool::new(false),
        }
    }

    fn finish(&self, lifecycle: ClaudeTerminalLifecycle, exit_code: Option<u32>) {
        if let Ok(mut state) = self.state.lock() {
            if state.lifecycle == ClaudeTerminalLifecycle::Running {
                state.lifecycle = lifecycle;
                state.exit_code = exit_code;
            }
            self.state_changed.notify_all();
            self.output_changed.notify_all();
        }
    }

    fn push_output(&self, bytes: Vec<u8>) -> bool {
        let Ok(mut output) = self.output.lock() else {
            self.finish(ClaudeTerminalLifecycle::IoFailed, None);
            self.cancellation.store(true, Ordering::SeqCst);
            return false;
        };
        if output.chunks.len() >= MAX_BUFFERED_TERMINAL_OUTPUT_CHUNKS
            || output.bytes.saturating_add(bytes.len()) > MAX_BUFFERED_TERMINAL_OUTPUT_BYTES
        {
            drop(output);
            self.finish(ClaudeTerminalLifecycle::OutputLimitExceeded, None);
            self.cancellation.store(true, Ordering::SeqCst);
            return false;
        }
        let sequence = output.next_sequence;
        output.next_sequence = output.next_sequence.saturating_add(1);
        output.bytes += bytes.len();
        output.chunks.push_back((sequence, bytes));
        self.output_changed.notify_all();
        true
    }

    fn close_output(&self) {
        self.output_closed.store(true, Ordering::SeqCst);
        self.output_changed.notify_all();
    }
}

struct SessionInner {
    id: ClaudeTerminalSessionId,
    master: Mutex<Option<Box<dyn portable_pty::MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    #[cfg(unix)]
    process_group_id: i32,
    #[cfg(windows)]
    job: Mutex<Option<windows_process::JobHandle>>,
    signals: Arc<SessionSignals>,
    cleaned: AtomicBool,
    profile_lease: Mutex<Option<RuntimeProfileSupervisorLease>>,
}

impl SessionInner {
    fn snapshot(&self) -> ClaudeTerminalSnapshot {
        let (lifecycle, exit_code) = self
            .signals
            .state
            .lock()
            .map(|state| (state.lifecycle, state.exit_code))
            .unwrap_or((ClaudeTerminalLifecycle::IoFailed, None));
        ClaudeTerminalSnapshot {
            session_id: self.id.clone(),
            lifecycle,
            exit_code,
        }
    }

    fn poll_exit(&self) -> bool {
        let status = match self.child.lock() {
            Ok(mut slot) => match slot.as_mut() {
                Some(child) => child.try_wait(),
                None => return true,
            },
            Err(_) => {
                self.signals.finish(ClaudeTerminalLifecycle::IoFailed, None);
                self.cancel_and_cleanup();
                return true;
            }
        };
        let status = match status {
            Ok(Some(status)) => status,
            Ok(None) => return false,
            Err(_) => {
                self.signals.finish(ClaudeTerminalLifecycle::IoFailed, None);
                self.cancel_and_cleanup();
                return true;
            }
        };
        let lifecycle = if status.success() {
            ClaudeTerminalLifecycle::Exited
        } else {
            ClaudeTerminalLifecycle::Crashed
        };
        self.signals.finish(lifecycle, Some(status.exit_code()));
        self.cleanup_after_exit();
        true
    }

    fn cleanup_after_exit(&self) {
        if self.cleaned.swap(true, Ordering::SeqCst) {
            return;
        }
        self.terminate_descendants();
        self.drop_terminal_io();
        if let Ok(mut child) = self.child.lock() {
            child.take();
        }
        self.release_profile_lease();
    }

    fn cancel_and_cleanup(&self) {
        self.signals.cancellation.store(true, Ordering::SeqCst);
        if self.cleaned.swap(true, Ordering::SeqCst) {
            return;
        }
        self.signals
            .finish(ClaudeTerminalLifecycle::Cancelled, None);
        self.terminate_descendants();
        self.drop_terminal_io();
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
                let deadline = Instant::now() + TERMINATION_GRACE;
                while Instant::now() < deadline {
                    if child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    thread::sleep(MONITOR_TICK);
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            slot.take();
        }
        self.release_profile_lease();
        self.signals.state_changed.notify_all();
        self.signals.output_changed.notify_all();
    }

    fn drop_terminal_io(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            writer.take();
        }
        if let Ok(mut master) = self.master.lock() {
            master.take();
        }
    }

    fn release_profile_lease(&self) {
        if let Ok(mut lease) = self.profile_lease.lock() {
            lease.take();
        }
    }

    fn terminate_descendants(&self) {
        #[cfg(unix)]
        {
            // portable-pty creates a new session before exec. Signal that
            // exact process group, never a caller-supplied or ambient pid.
            let _ = unsafe { libc::kill(-self.process_group_id, libc::SIGTERM) };
            thread::sleep(TERMINATION_GRACE);
            let _ = unsafe { libc::kill(-self.process_group_id, libc::SIGKILL) };
        }
        #[cfg(windows)]
        if let Ok(mut job) = self.job.lock() {
            // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE terminates every descendant.
            job.take();
        }
    }
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        self.cancel_and_cleanup();
    }
}

pub struct ClaudeTerminalSession {
    inner: Arc<SessionInner>,
}

impl fmt::Debug for ClaudeTerminalSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudeTerminalSession")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl ClaudeTerminalSession {
    pub fn id(&self) -> &ClaudeTerminalSessionId {
        &self.inner.id
    }

    pub fn snapshot(&self) -> ClaudeTerminalSnapshot {
        self.inner.snapshot()
    }

    pub fn write_input(&self, bytes: &[u8]) -> Result<(), ClaudeTerminalError> {
        if bytes.is_empty() || bytes.len() > MAX_TERMINAL_INPUT_BYTES {
            return Err(terminal_error(ClaudeTerminalErrorCode::InputLimitExceeded));
        }
        if self.snapshot().lifecycle != ClaudeTerminalLifecycle::Running {
            return Err(terminal_error(ClaudeTerminalErrorCode::NotActive));
        }
        let mut writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))?;
        let writer = writer
            .as_mut()
            .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::NotActive))?;
        writer
            .write_all(bytes)
            .and_then(|_| writer.flush())
            .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))
    }

    pub fn resize(
        &self,
        columns: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), ClaudeTerminalError> {
        if columns == 0
            || rows == 0
            || self.snapshot().lifecycle != ClaudeTerminalLifecycle::Running
        {
            return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
        }
        let master = self
            .inner
            .master
            .lock()
            .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))?;
        master
            .as_ref()
            .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::NotActive))?
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width,
                pixel_height,
            })
            .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))
    }

    /// Returns one transient bounded PTY chunk. The bytes are base64 encoded
    /// only for transport across the future Tauri command boundary; they are
    /// removed from the in-memory queue as soon as this method returns.
    pub fn read_output(
        &self,
        timeout: Duration,
    ) -> Result<Option<ClaudeTerminalOutput>, ClaudeTerminalError> {
        if timeout > MAX_TERMINAL_READ_WAIT {
            return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
        let mut output = self
            .inner
            .signals
            .output
            .lock()
            .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))?;
        loop {
            if let Some((sequence, bytes)) = output.chunks.pop_front() {
                output.bytes = output.bytes.saturating_sub(bytes.len());
                return Ok(Some(ClaudeTerminalOutput {
                    session_id: self.inner.id.clone(),
                    sequence,
                    data_base64: BASE64_STANDARD.encode(bytes),
                }));
            }
            if (self.snapshot().lifecycle.terminal()
                && self.inner.signals.output_closed.load(Ordering::SeqCst))
                || timeout.is_zero()
            {
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let (next, result) = self
                .inner
                .signals
                .output_changed
                .wait_timeout(output, remaining)
                .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))?;
            output = next;
            if result.timed_out() && output.chunks.is_empty() {
                return Ok(None);
            }
        }
    }

    pub fn cancel(&self) -> Result<ClaudeTerminalSnapshot, ClaudeTerminalError> {
        self.inner.cancel_and_cleanup();
        Ok(self.snapshot())
    }

    pub fn wait(&self, timeout: Duration) -> Result<ClaudeTerminalSnapshot, ClaudeTerminalError> {
        if timeout > MAX_TERMINAL_SESSION_WAIT {
            return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
        let mut state = self
            .inner
            .signals
            .state
            .lock()
            .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))?;
        while !state.lifecycle.terminal() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(terminal_error(ClaudeTerminalErrorCode::WaitTimedOut));
            }
            let (next, result) = self
                .inner
                .signals
                .state_changed
                .wait_timeout(state, remaining)
                .map_err(|_| terminal_error(ClaudeTerminalErrorCode::IoFailed))?;
            state = next;
            if result.timed_out() && !state.lifecycle.terminal() {
                return Err(terminal_error(ClaudeTerminalErrorCode::WaitTimedOut));
            }
        }
        let snapshot = ClaudeTerminalSnapshot {
            session_id: self.inner.id.clone(),
            lifecycle: state.lifecycle,
            exit_code: state.exit_code,
        };
        Ok(snapshot)
    }
}

/// Launches only the currently active, sealed-package-verified executable.
/// There is intentionally no overload accepting a path or executable name.
pub fn start_terminal_session(
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
    spec: ClaudeTerminalLaunchSpec,
) -> Result<ClaudeTerminalSession, ClaudeTerminalError> {
    validate_selection(package, profile)?;
    let profile_lease = profile
        .acquire_supervisor_lease()
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidProfile))?;
    let executable = package
        .verified_active_executable_path()
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidSelection))?;
    let executable = validate_executable(&executable)?;
    let working_directory = validate_directory(&spec.working_directory)?;
    let path_extensions = validate_path_extensions(&spec.path_extensions)?;
    if spec.columns == 0 || spec.rows == 0 {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
    }

    let pty_system = native_pty_system();
    let mut pair = pty_system
        .openpty(PtySize {
            rows: spec.rows,
            cols: spec.columns,
            pixel_width: spec.pixel_width,
            pixel_height: spec.pixel_height,
        })
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::SpawnFailed))?;
    let mut command = CommandBuilder::new(&executable);
    command.args(spec.mode.arguments());
    command.env_clear();
    command.cwd(&working_directory);
    for (key, value) in code_owned_environment(&executable, profile, &path_extensions)? {
        command.env(key, value);
    }
    #[cfg(test)]
    if spec.mode == ClaudeTerminalMode::Fixture {
        if let Some(mode) = FIXTURE_MODE.with(|value| value.borrow().clone()) {
            command.env("ALFRED_CLAUDE_PTY_FIXTURE", mode);
        }
        if let Some(path) = FIXTURE_SENTINEL.with(|value| value.borrow().clone()) {
            command.env("ALFRED_CLAUDE_PTY_SENTINEL", path);
        }
    }
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::SpawnFailed))?;
    drop(pair.slave);

    #[cfg(unix)]
    let process_group_id = pair
        .master
        .process_group_leader()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| {
            // Refuse session ownership without the PTY-reported process group.
            // The spawned portable-pty child is a session leader, so its own
            // pid is still the safest best-effort cleanup target on this
            // already-failing setup path.
            if let Some(pid) = child
                .process_id()
                .and_then(|pid| i32::try_from(pid).ok())
                .filter(|pid| *pid > 0)
            {
                let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
            let _ = child.kill();
            let _ = child.wait();
            terminal_error(ClaudeTerminalErrorCode::ProcessTreeUnavailable)
        })?;
    #[cfg(windows)]
    let job = windows_process::JobHandle::assign(child.as_ref()).map_err(|_| {
        let _ = child.kill();
        let _ = child.wait();
        terminal_error(ClaudeTerminalErrorCode::ProcessTreeUnavailable)
    })?;

    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(_) => {
            #[cfg(unix)]
            cleanup_spawn_failure(child.as_mut(), process_group_id);
            #[cfg(windows)]
            cleanup_spawn_failure(child.as_mut(), job);
            return Err(terminal_error(ClaudeTerminalErrorCode::IoFailed));
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(_) => {
            drop(reader);
            #[cfg(unix)]
            cleanup_spawn_failure(child.as_mut(), process_group_id);
            #[cfg(windows)]
            cleanup_spawn_failure(child.as_mut(), job);
            return Err(terminal_error(ClaudeTerminalErrorCode::IoFailed));
        }
    };
    let signals = Arc::new(SessionSignals::new());
    let inner = Arc::new(SessionInner {
        id: ClaudeTerminalSessionId::generate(),
        master: Mutex::new(Some(pair.master)),
        writer: Mutex::new(Some(writer)),
        child: Mutex::new(Some(child)),
        #[cfg(unix)]
        process_group_id,
        #[cfg(windows)]
        job: Mutex::new(Some(job)),
        signals: Arc::clone(&signals),
        cleaned: AtomicBool::new(false),
        profile_lease: Mutex::new(Some(profile_lease)),
    });
    spawn_output_reader(reader, Arc::clone(&signals));
    spawn_monitor(Arc::downgrade(&inner));
    Ok(ClaudeTerminalSession { inner })
}

pub(super) fn validate_selection(
    package: &RuntimePackageSelection,
    profile: &RuntimeProfile,
) -> Result<(), ClaudeTerminalError> {
    profile
        .revalidate_for_launch()
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidProfile))?;
    let expectation = package.expectation();
    let binding = profile.binding();
    artifact_for_target(expectation.target())
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidSelection))?;
    if profile.lifecycle() != RuntimeProfileLifecycle::Active
        || current_runtime_target() != Some(expectation.target())
        || expectation.product() != AgentProductId::ClaudeCodeSubscription
        || expectation.product() != binding.product()
        || expectation.runtime_id() != binding.runtime_id()
        || expectation.runtime_version() != CLAUDE_CODE_RUNTIME_VERSION
        || expectation.runtime_version() != binding.runtime_version()
    {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidSelection));
    }
    Ok(())
}

fn current_runtime_target() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some("aarch64-apple-darwin");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Some("x86_64-apple-darwin");
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
    return Some("aarch64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    return Some("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "musl"))]
    return Some("aarch64-unknown-linux-musl");
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "musl"))]
    return Some("x86_64-unknown-linux-musl");
    #[cfg(all(windows, target_arch = "aarch64"))]
    return Some("aarch64-pc-windows-msvc");
    #[cfg(all(windows, target_arch = "x86_64"))]
    return Some("x86_64-pc-windows-msvc");
    #[allow(unreachable_code)]
    None
}

fn validate_executable(path: &Path) -> Result<PathBuf, ClaudeTerminalError> {
    if !path.is_absolute() {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidSelection));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidSelection))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidSelection));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidSelection))?;
    if canonical != path {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidSelection));
    }
    Ok(canonical)
}

fn validate_directory(path: &Path) -> Result<PathBuf, ClaudeTerminalError> {
    if !path.is_absolute() {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
    }
    path.canonicalize()
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))
}

fn validate_path_extensions(paths: &[PathBuf]) -> Result<Vec<PathBuf>, ClaudeTerminalError> {
    if paths.len() > MAX_TERMINAL_PATH_EXTENSIONS {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
    }
    let mut validated = Vec::with_capacity(paths.len());
    for path in paths {
        let canonical = validate_directory(path)?;
        if &canonical != path || validated.contains(&canonical) {
            return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
        }
        validated.push(canonical);
    }
    Ok(validated)
}

fn spawn_output_reader(mut reader: Box<dyn Read + Send>, signals: Arc<SessionSignals>) {
    thread::spawn(move || {
        let mut buffer = vec![0u8; MAX_TERMINAL_OUTPUT_CHUNK_BYTES];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    signals.close_output();
                    return;
                }
                Ok(read) => {
                    if !signals.push_output(buffer[..read].to_vec()) {
                        signals.close_output();
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    signals.finish(ClaudeTerminalLifecycle::IoFailed, None);
                    signals.cancellation.store(true, Ordering::SeqCst);
                    signals.close_output();
                    return;
                }
            }
        }
    });
}

fn spawn_monitor(inner: Weak<SessionInner>) {
    thread::spawn(move || loop {
        let Some(session) = inner.upgrade() else {
            return;
        };
        if session.signals.cancellation.load(Ordering::SeqCst) {
            session.cancel_and_cleanup();
            return;
        }
        if session.poll_exit() {
            return;
        }
        drop(session);
        thread::sleep(MONITOR_TICK);
    });
}

#[cfg(unix)]
fn cleanup_spawn_failure(child: &mut dyn portable_pty::Child, process_group_id: i32) {
    let _ = unsafe { libc::kill(-process_group_id, libc::SIGTERM) };
    thread::sleep(TERMINATION_GRACE);
    let _ = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn cleanup_spawn_failure(child: &mut dyn portable_pty::Child, job: windows_process::JobHandle) {
    // Closing a kill-on-close Job Object terminates descendants as well as
    // the immediate PTY child when setup fails before session ownership moves.
    drop(job);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn code_owned_environment(
    executable: &Path,
    profile: &RuntimeProfile,
    path_extensions: &[PathBuf],
) -> Result<Vec<(OsString, OsString)>, ClaudeTerminalError> {
    let package_bin = executable
        .parent()
        .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidSelection))?;
    let config_root = profile
        .environment_roots()
        .get(RuntimeEnvironmentVariable::ClaudeConfigDir)
        .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidProfile))?;
    let mut path_entries = vec![package_bin];
    #[cfg(target_os = "macos")]
    for candidate in [Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")] {
        if fs::symlink_metadata(candidate)
            .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir())
            .unwrap_or(false)
        {
            path_entries.push(candidate);
        }
    }
    path_entries.extend([
        Path::new("/usr/bin"),
        Path::new("/bin"),
        Path::new("/usr/sbin"),
        Path::new("/sbin"),
    ]);
    path_entries.extend(path_extensions.iter().map(PathBuf::as_path));
    let path = std::env::join_paths(path_entries)
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
    Ok(vec![
        ("HOME".into(), profile.launch_home_root().as_os_str().into()),
        (
            "TMPDIR".into(),
            profile.launch_temp_root().as_os_str().into(),
        ),
        ("TMP".into(), profile.launch_temp_root().as_os_str().into()),
        ("TEMP".into(), profile.launch_temp_root().as_os_str().into()),
        ("CLAUDE_CONFIG_DIR".into(), config_root.as_os_str().into()),
        ("PATH".into(), path),
        ("SHELL".into(), OsString::from("/bin/sh")),
        ("TERM".into(), OsString::from("xterm-256color")),
        ("COLORTERM".into(), OsString::from("truecolor")),
        ("DISABLE_AUTOUPDATER".into(), OsString::from("1")),
        ("DISABLE_UPDATES".into(), OsString::from("1")),
    ])
}

#[cfg(windows)]
fn code_owned_environment(
    executable: &Path,
    profile: &RuntimeProfile,
    path_extensions: &[PathBuf],
) -> Result<Vec<(OsString, OsString)>, ClaudeTerminalError> {
    let package_bin = executable
        .parent()
        .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidSelection))?;
    let config_root = profile
        .environment_roots()
        .get(RuntimeEnvironmentVariable::ClaudeConfigDir)
        .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidProfile))?;
    let system_root = windows_process::windows_directory()
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
    if !system_root.is_absolute()
        || system_root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch));
    }
    let system32 = system_root.join("System32");
    let mut path_entries = vec![
        package_bin.to_path_buf(),
        system32.clone(),
        system_root.clone(),
        system32.join("Wbem"),
        system32.join("WindowsPowerShell/v1.0"),
    ];
    path_entries.extend(path_extensions.iter().cloned());
    let path = std::env::join_paths(path_entries)
        .map_err(|_| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
    let home = child_environment_path(profile.launch_home_root());
    let temp = child_environment_path(profile.launch_temp_root());
    let system_root_value = child_environment_path(&system_root);
    let system_drive_root = system_root
        .parent()
        .ok_or_else(|| terminal_error(ClaudeTerminalErrorCode::InvalidLaunch))?;
    let program_files = child_environment_path(&system_drive_root.join("Program Files"));
    let processor_architecture = match std::env::consts::ARCH {
        "x86" => "x86",
        "x86_64" => "AMD64",
        "aarch64" => "ARM64",
        _ => return Err(terminal_error(ClaudeTerminalErrorCode::InvalidLaunch)),
    };
    let mut environment = vec![
        ("HOME".into(), home.clone()),
        ("USERPROFILE".into(), home.clone()),
        (
            "APPDATA".into(),
            child_environment_path(&profile.launch_home_root().join("AppData/Roaming")),
        ),
        (
            "LOCALAPPDATA".into(),
            child_environment_path(&profile.launch_home_root().join("AppData/Local")),
        ),
        (
            "PROGRAMDATA".into(),
            child_environment_path(&system_drive_root.join("ProgramData")),
        ),
        ("ProgramFiles".into(), program_files.clone()),
        ("ProgramW6432".into(), program_files),
        (
            "PROCESSOR_ARCHITECTURE".into(),
            OsString::from(processor_architecture),
        ),
        (
            "NUMBER_OF_PROCESSORS".into(),
            thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1)
                .to_string()
                .into(),
        ),
        ("TEMP".into(), temp.clone()),
        ("TMP".into(), temp),
        (
            "CLAUDE_CONFIG_DIR".into(),
            child_environment_path(config_root),
        ),
        ("PATH".into(), path),
        ("SystemRoot".into(), system_root_value.clone()),
        ("windir".into(), system_root_value.clone()),
        (
            "COMSPEC".into(),
            child_environment_path(&system32.join("cmd.exe")),
        ),
        (
            "PATHEXT".into(),
            OsString::from(".COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC"),
        ),
        ("TERM".into(), OsString::from("xterm-256color")),
        ("COLORTERM".into(), OsString::from("truecolor")),
        ("DISABLE_AUTOUPDATER".into(), OsString::from("1")),
        ("DISABLE_UPDATES".into(), OsString::from("1")),
    ];
    let root_text = system_root_value.to_string_lossy();
    if root_text.len() >= 2 && root_text.as_bytes()[1] == b':' {
        environment.push(("SystemDrive".into(), OsString::from(&root_text[..2])));
    }
    let home_text = home.to_string_lossy();
    if home_text.len() >= 3 && home_text.as_bytes()[1] == b':' {
        environment.push(("HOMEDRIVE".into(), OsString::from(&home_text[..2])));
        environment.push(("HOMEPATH".into(), OsString::from(&home_text[2..])));
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

#[cfg(test)]
thread_local! {
    static FIXTURE_MODE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static FIXTURE_SENTINEL: std::cell::RefCell<Option<OsString>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct TerminalFixtureReset;

#[cfg(test)]
impl Drop for TerminalFixtureReset {
    fn drop(&mut self) {
        FIXTURE_MODE.with(|value| value.borrow_mut().take());
        FIXTURE_SENTINEL.with(|value| value.borrow_mut().take());
    }
}

#[cfg(test)]
pub(super) fn with_terminal_fixture<T>(
    mode: &str,
    sentinel: Option<&Path>,
    operation: impl FnOnce() -> T,
) -> T {
    FIXTURE_MODE.with(|value| *value.borrow_mut() = Some(mode.into()));
    FIXTURE_SENTINEL
        .with(|value| *value.borrow_mut() = sentinel.map(|path| path.as_os_str().to_owned()));
    let _reset = TerminalFixtureReset;
    operation()
}

#[cfg(windows)]
mod windows_process {
    use portable_pty::Child;
    use std::ffi::c_void;
    use std::io;
    use std::mem;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::ptr;

    type Handle = *mut c_void;
    type Bool = i32;
    type Dword = u32;
    type LargeInteger = i64;

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

    unsafe impl Send for JobHandle {}

    impl JobHandle {
        pub(super) fn assign(child: &dyn Child) -> io::Result<Self> {
            let process = child.as_raw_handle().ok_or_else(|| {
                io::Error::new(io::ErrorKind::Unsupported, "PTY child handle unavailable")
            })? as Handle;
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
                    || AssignProcessToJobObject(job, process) == 0
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
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    pub(super) fn windows_directory() -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 32_768];
        let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 || length as usize >= buffer.len() {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(length as usize);
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
    }
}
