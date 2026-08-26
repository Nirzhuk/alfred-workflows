//! Noninteractive `claude auth status` through the shared managed supervisor.
//!
//! The pinned command emits documented JSON and exits. The supervisor still
//! supplies sealed-package revalidation, profile isolation, bounded output,
//! cancellation, deadline handling, and process-tree cleanup. Interactive
//! login/logout remain on the PTY path because their user-facing terminal
//! output must not be parsed or intercepted.

use super::auth::{parse_auth_status, ClaudeAuthStatus, MAX_AUTH_STATUS_BYTES};
use super::terminal::validate_selection;
use crate::agent_accounts::runtime_profile::{RuntimeEnvironmentVariable, RuntimeProfile};
use crate::agents::managed_runtime::{
    ManagedRuntimeCancellation, ManagedRuntimeLaunchSpec, ManagedRuntimeLifecycle,
    ManagedRuntimeSupervisor, RuntimeReadinessProbe, RuntimeShutdownHook, RuntimeStdoutPolicy,
};
use crate::agents::runtime_package::RuntimePackageSelection;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

const STATUS_TIMEOUT: Duration = Duration::from_secs(15);
const STATUS_DRAIN_TICK: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeStatusErrorCode {
    InvalidProfile,
    LaunchRejected,
    OutputLimitExceeded,
    OutputInvalid,
    CommandFailed,
}

impl ClaudeStatusErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidProfile => "claude_status_profile_invalid",
            Self::LaunchRejected => "claude_status_launch_rejected",
            Self::OutputLimitExceeded => "claude_status_output_limit_exceeded",
            Self::OutputInvalid => "claude_status_output_invalid",
            Self::CommandFailed => "claude_status_command_failed",
        }
    }
}

pub struct ClaudeStatusError(ClaudeStatusErrorCode);

impl ClaudeStatusError {
    pub fn code(&self) -> ClaudeStatusErrorCode {
        self.0
    }
}

impl fmt::Debug for ClaudeStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl fmt::Display for ClaudeStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl std::error::Error for ClaudeStatusError {}

fn status_error(code: ClaudeStatusErrorCode) -> ClaudeStatusError {
    ClaudeStatusError(code)
}

#[derive(Clone, Default)]
pub struct ClaudeAuthStatusService {
    supervisor: ManagedRuntimeSupervisor,
}

impl fmt::Debug for ClaudeAuthStatusService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ClaudeAuthStatusService")
    }
}

impl ClaudeAuthStatusService {
    pub fn new(supervisor: ManagedRuntimeSupervisor) -> Self {
        Self { supervisor }
    }

    pub fn query(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        cancellation: ManagedRuntimeCancellation,
    ) -> Result<ClaudeAuthStatus, ClaudeStatusError> {
        self.query_with_spec(package, profile, cancellation, status_spec(profile)?)
    }

    fn query_with_spec(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        cancellation: ManagedRuntimeCancellation,
        spec: ManagedRuntimeLaunchSpec,
    ) -> Result<ClaudeAuthStatus, ClaudeStatusError> {
        validate_selection(package, profile)
            .map_err(|_| status_error(ClaudeStatusErrorCode::LaunchRejected))?;
        let handle = self
            .supervisor
            .launch(package, profile, spec, cancellation)
            .map_err(|_| status_error(ClaudeStatusErrorCode::LaunchRejected))?;
        let mut output = Vec::new();
        loop {
            if let Some(frame) = handle
                .read_stdout_frame(STATUS_DRAIN_TICK)
                .map_err(|_| status_error(ClaudeStatusErrorCode::CommandFailed))?
            {
                append_frame(&mut output, &frame)?;
                continue;
            }
            if handle.snapshot().lifecycle == ManagedRuntimeLifecycle::Exited
                || handle.snapshot().lifecycle == ManagedRuntimeLifecycle::Failed
            {
                while let Some(frame) = handle
                    .read_stdout_frame(Duration::ZERO)
                    .map_err(|_| status_error(ClaudeStatusErrorCode::CommandFailed))?
                {
                    append_frame(&mut output, &frame)?;
                }
                break;
            }
        }
        let status = parse_status_process_output(&output)
            .map_err(|_| status_error(ClaudeStatusErrorCode::OutputInvalid))?;
        let snapshot = handle.snapshot();
        let exit_code = snapshot
            .termination
            .and_then(|termination| termination.exit_code);
        let expected_exit = if status.logged_in {
            snapshot.lifecycle == ManagedRuntimeLifecycle::Exited && exit_code == Some(0)
        } else {
            // The documented command exits 1 when logged out. Some publisher
            // builds have returned 0 with `loggedIn:false`; both are truthful.
            matches!(exit_code, Some(0 | 1))
        };
        if !expected_exit {
            return Err(status_error(ClaudeStatusErrorCode::CommandFailed));
        }
        Ok(status)
    }

    #[cfg(test)]
    pub(super) fn query_fixture(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        fixture_mode: &str,
    ) -> Result<ClaudeAuthStatus, ClaudeStatusError> {
        const FIXTURE_TEST: &str =
            "agents::native::providers::claude::subscription_tests::claude_managed_fixture_child";
        let config_root = profile
            .environment_roots()
            .get(RuntimeEnvironmentVariable::ClaudeConfigDir)
            .ok_or_else(|| status_error(ClaudeStatusErrorCode::InvalidProfile))?;
        let environment = BTreeMap::from([
            ("DISABLE_UPDATES".into(), "1".into()),
            ("DISABLE_AUTOUPDATER".into(), "1".into()),
            ("ALFRED_CLAUDE_STATUS_FIXTURE".into(), fixture_mode.into()),
        ]);
        let spec = ManagedRuntimeLaunchSpec::new(
            vec![
                "--quiet".into(),
                "--exact".into(),
                FIXTURE_TEST.into(),
                "--nocapture".into(),
            ],
            RuntimeReadinessProbe::stdout_line_equals("{")
                .map_err(|_| status_error(ClaudeStatusErrorCode::LaunchRejected))?,
            RuntimeShutdownHook::CloseStdin,
            RuntimeStdoutPolicy::TypedFramesFailClosed,
        )
        .with_working_directory(config_root)
        .with_environment(environment)
        .with_startup_timeout(Duration::from_secs(2))
        .with_shutdown_timeout(Duration::from_millis(250))
        .with_runtime_deadline(Duration::from_secs(2));
        self.query_with_spec(package, profile, ManagedRuntimeCancellation::new(), spec)
    }
}

fn parse_status_process_output(
    output: &[u8],
) -> Result<ClaudeAuthStatus, super::auth::ClaudeAuthStatusError> {
    let start = output
        .iter()
        .position(|byte| *byte == b'{')
        .ok_or(super::auth::ClaudeAuthStatusError::OutputInvalid)?;
    let mut values =
        serde_json::Deserializer::from_slice(&output[start..]).into_iter::<serde_json::Value>();
    let value = values
        .next()
        .transpose()
        .map_err(|_| super::auth::ClaudeAuthStatusError::OutputInvalid)?
        .ok_or(super::auth::ClaudeAuthStatusError::OutputInvalid)?;
    let normalized = serde_json::to_vec(&value)
        .map_err(|_| super::auth::ClaudeAuthStatusError::OutputInvalid)?;
    parse_auth_status(&normalized)
}

fn status_spec(profile: &RuntimeProfile) -> Result<ManagedRuntimeLaunchSpec, ClaudeStatusError> {
    let config_root = profile
        .environment_roots()
        .get(RuntimeEnvironmentVariable::ClaudeConfigDir)
        .ok_or_else(|| status_error(ClaudeStatusErrorCode::InvalidProfile))?;
    let environment = BTreeMap::from([
        ("DISABLE_UPDATES".into(), "1".into()),
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
    ]);
    Ok(ManagedRuntimeLaunchSpec::new(
        vec!["auth".into(), "status".into()],
        RuntimeReadinessProbe::stdout_line_equals("{")
            .map_err(|_| status_error(ClaudeStatusErrorCode::LaunchRejected))?,
        RuntimeShutdownHook::CloseStdin,
        RuntimeStdoutPolicy::TypedFramesFailClosed,
    )
    .with_working_directory(config_root)
    .with_environment(environment)
    .with_startup_timeout(Duration::from_secs(5))
    .with_shutdown_timeout(Duration::from_millis(500))
    .with_runtime_deadline(STATUS_TIMEOUT))
}

fn append_frame(output: &mut Vec<u8>, frame: &[u8]) -> Result<(), ClaudeStatusError> {
    let additional = frame.len().saturating_add(1);
    if output.len().saturating_add(additional) > MAX_AUTH_STATUS_BYTES {
        return Err(status_error(ClaudeStatusErrorCode::OutputLimitExceeded));
    }
    output.extend_from_slice(frame);
    output.push(b'\n');
    Ok(())
}
