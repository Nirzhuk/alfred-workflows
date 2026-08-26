//! Managed, unmodified Claude Code subscription product boundary.
//!
//! This product is separate from the direct `claude_api` runtime in the parent
//! module. It never accepts an API key, never reads a Claude credential store,
//! and never substitutes `claude -p` or the Agent SDK for the publisher's TUI.

use super::auth::ClaudeAuthStatus;
use super::status::{ClaudeAuthStatusService, ClaudeStatusError};
use super::terminal::{
    start_terminal_session, ClaudeTerminalError, ClaudeTerminalLaunchSpec, ClaudeTerminalMode,
    ClaudeTerminalSession,
};
use crate::agent_accounts::runtime_profile::RuntimeProfile;
use crate::agents::managed_runtime::{ManagedRuntimeCancellation, ManagedRuntimeSupervisor};
use crate::agents::native::{
    CapabilityReportStatus, NativeErrorCode, NativeRuntimeError, NativeRuntimeRegistry,
};
use crate::agents::runtime_package::RuntimePackageSelection;
use serde::Serialize;
use std::path::Path;

pub const SUBSCRIPTION_PRODUCT_ID: &str = "claude_code_subscription";
pub const SUBSCRIPTION_RUNTIME_ID: &str = "claude_code_managed";
pub const SUBSCRIPTION_RUNTIME_VERSION: &str = super::package::CLAUDE_CODE_RUNTIME_VERSION;

pub const COMMERCIAL_TERMS_BLOCKED_CODE: &str = "claude_commercial_terms_unconfirmed";
pub const PACKAGE_INTEGRATION_BLOCKED_CODE: &str = "claude_managed_package_integration_missing";
pub const PUBLISHER_VERIFIER_BLOCKED_CODE: &str =
    "claude_publisher_verification_integration_missing";
pub const PACKAGED_NO_CLI_SMOKE_BLOCKED_CODE: &str = "claude_packaged_no_cli_smoke_missing";
pub const WORKFLOW_RENDERER_APPROVAL_BLOCKED_CODE: &str =
    "claude_native_workflow_renderer_approval_missing";

const REGISTRATION_BLOCKERS: [&str; 4] = [
    COMMERCIAL_TERMS_BLOCKED_CODE,
    PACKAGE_INTEGRATION_BLOCKED_CODE,
    PUBLISHER_VERIFIER_BLOCKED_CODE,
    PACKAGED_NO_CLI_SMOKE_BLOCKED_CODE,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSubscriptionReleaseGate {
    pub gate: &'static str,
    pub status: CapabilityReportStatus,
    pub evidence: &'static str,
}

pub fn subscription_release_gates() -> Vec<ClaudeSubscriptionReleaseGate> {
    vec![
        ClaudeSubscriptionReleaseGate {
            gate: "runtime_contract",
            status: CapabilityReportStatus::Supported,
            evidence: "exact unmodified Claude Code 2.1.246 binary; provider-owned PTY onboarding/auth; no claude -p or Agent SDK renderer",
        },
        ClaudeSubscriptionReleaseGate {
            gate: "profile_contract",
            status: CapabilityReportStatus::Supported,
            evidence: "account-scoped CLAUDE_CONFIG_DIR and isolated HOME/TEMP roots; Alfred has no credential/profile-file/keychain reader",
        },
        ClaudeSubscriptionReleaseGate {
            gate: "commercial_terms",
            status: CapabilityReportStatus::Blocked,
            evidence: COMMERCIAL_TERMS_BLOCKED_CODE,
        },
        ClaudeSubscriptionReleaseGate {
            gate: "package_integration",
            status: CapabilityReportStatus::Blocked,
            evidence: PACKAGE_INTEGRATION_BLOCKED_CODE,
        },
        ClaudeSubscriptionReleaseGate {
            gate: "publisher_verifier",
            status: CapabilityReportStatus::Blocked,
            evidence: PUBLISHER_VERIFIER_BLOCKED_CODE,
        },
        ClaudeSubscriptionReleaseGate {
            gate: "packaged_no_cli_smoke",
            status: CapabilityReportStatus::Blocked,
            evidence: PACKAGED_NO_CLI_SMOKE_BLOCKED_CODE,
        },
        ClaudeSubscriptionReleaseGate {
            gate: "native_workflow_renderer",
            status: CapabilityReportStatus::Blocked,
            evidence: WORKFLOW_RENDERER_APPROVAL_BLOCKED_CODE,
        },
    ]
}

pub fn subscription_registration_blockers() -> &'static [&'static str] {
    &REGISTRATION_BLOCKERS
}

/// Production registration remains structurally fail-closed even though the
/// backend runtime primitives can be exercised with verified fake packages.
pub fn register_subscription(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        REGISTRATION_BLOCKERS.join("; "),
        false,
    ))
}

#[derive(Clone, Default)]
pub struct ClaudeCodeSubscriptionRuntime {
    status: ClaudeAuthStatusService,
}

impl std::fmt::Debug for ClaudeCodeSubscriptionRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ClaudeCodeSubscriptionRuntime")
    }
}

impl ClaudeCodeSubscriptionRuntime {
    pub fn new(supervisor: ManagedRuntimeSupervisor) -> Self {
        Self {
            status: ClaudeAuthStatusService::new(supervisor),
        }
    }

    pub fn auth_status(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        cancellation: ManagedRuntimeCancellation,
    ) -> Result<ClaudeAuthStatus, ClaudeStatusError> {
        self.status.query(package, profile, cancellation)
    }

    pub fn start_terminal(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        spec: ClaudeTerminalLaunchSpec,
    ) -> Result<ClaudeTerminalSession, ClaudeTerminalError> {
        start_terminal_session(package, profile, spec)
    }

    pub fn start_onboarding(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        working_directory: &Path,
        columns: u16,
        rows: u16,
    ) -> Result<ClaudeTerminalSession, ClaudeTerminalError> {
        self.start_terminal(
            package,
            profile,
            ClaudeTerminalLaunchSpec::new(
                ClaudeTerminalMode::Onboarding,
                working_directory,
                columns,
                rows,
            ),
        )
    }

    pub fn start_auth_login(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        working_directory: &Path,
        columns: u16,
        rows: u16,
    ) -> Result<ClaudeTerminalSession, ClaudeTerminalError> {
        self.start_terminal(
            package,
            profile,
            ClaudeTerminalLaunchSpec::new(
                ClaudeTerminalMode::AuthLogin,
                working_directory,
                columns,
                rows,
            ),
        )
    }

    pub fn start_auth_logout(
        &self,
        package: &RuntimePackageSelection,
        profile: &RuntimeProfile,
        working_directory: &Path,
        columns: u16,
        rows: u16,
    ) -> Result<ClaudeTerminalSession, ClaudeTerminalError> {
        self.start_terminal(
            package,
            profile,
            ClaudeTerminalLaunchSpec::new(
                ClaudeTerminalMode::AuthLogout,
                working_directory,
                columns,
                rows,
            ),
        )
    }
}
