//! The Copilot SDK seam.
//!
//! [`CopilotTransport`] is the only place the `github-copilot-sdk` crate would
//! be touched. Keeping it a trait means every mapping, bound, approval path,
//! and cancellation path below is exercised by fixtures without a child
//! process, a network call, or a Copilot seat.

use super::auth::CopilotAccessToken;
use super::entitlement::CopilotSessionRejection;
use super::events::CopilotSdkEvent;
use crate::agents::native::{
    AlfredToolResult, NativeErrorCode, NativeRuntimeError, NativeTurnRequest,
};
use std::path::PathBuf;

/// How the Copilot CLI runtime is located.
///
/// Mirrors the Rust SDK's documented resolution order. `PATH` is deliberately
/// absent: the SDK states "There is no PATH scanning", and native mode must not
/// silently adopt whatever `copilot` a user happens to have installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotRuntimeSource {
    /// The CLI embedded in the compiled crate by the `bundled-cli` feature and
    /// extracted to the per-user cache keyed by the pinned version.
    Bundled { version: String },
    /// An explicit override, for development only.
    ExplicitPath { path: PathBuf, version: String },
    /// The SDK dependency/runtime has not been packaged into this build.
    Unavailable { expected_version: String },
}

impl CopilotRuntimeSource {
    pub fn version(&self) -> &str {
        match self {
            Self::Bundled { version } | Self::ExplicitPath { version, .. } => version,
            Self::Unavailable { expected_version } => expected_version,
        }
    }

    /// True when Alfred owns the runtime's lifecycle and version. Plan 037's
    /// user-experience gate ("no manual CLI installation") requires this.
    pub fn is_alfred_managed(&self) -> bool {
        matches!(self, Self::Bundled { .. })
    }
}

/// Strict SDK configuration the real transport must apply.
///
/// This corresponds to `ClientMode::Empty`, `with_github_token(...)`, and
/// `with_use_logged_in_user(false)` in `github-copilot-sdk` 1.0.11. Only
/// Alfred-registered custom tools are admitted; built-in and MCP tools never
/// reach the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotSessionPolicy {
    pub client_mode_empty: bool,
    pub use_logged_in_user: bool,
    pub available_tools: Vec<String>,
}

impl CopilotSessionPolicy {
    pub fn alfred_boundary() -> Self {
        Self {
            client_mode_empty: true,
            use_logged_in_user: false,
            available_tools: [
                "alfred_file_read",
                "alfred_file_write",
                "alfred_file_edit",
                "alfred_directory_list",
                "alfred_shell",
            ]
            .into_iter()
            .map(|name| format!("custom:{name}"))
            .collect(),
        }
    }
}

/// The reply Alfred sends back for one SDK permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotPermissionReply {
    Allow,
    Deny,
}

/// One live Copilot session, driven synchronously by the native runtime.
///
/// Tool execution is **Alfred's**: the session is configured with Copilot's own
/// filesystem/shell tools filtered off and Alfred's tools registered through the
/// SDK's typed tool registration, so a tool call arrives as a
/// `tool.invocation_requested` event and its result goes back through
/// [`Self::respond_tool`]. Copilot never executes a workspace tool itself, and
/// nothing runs twice.
pub trait CopilotSession: Send {
    /// Blocks for the next SDK event. `Ok(None)` means the stream ended.
    fn next_event(&mut self) -> Result<Option<CopilotSdkEvent>, NativeRuntimeError>;
    /// Returns an Alfred-executed tool result to Copilot.
    fn respond_tool(
        &mut self,
        invocation_id: &str,
        result: &AlfredToolResult,
    ) -> Result<(), NativeRuntimeError>;
    /// Answers a Copilot-internal `permission.requested`.
    ///
    /// The runtime always answers `Deny` here: Alfred's approval handler is the
    /// only approval authority, and allowing a Copilot-internal permission
    /// would let a tool run outside the Alfred boundary.
    fn respond_permission(
        &mut self,
        request_id: &str,
        reply: CopilotPermissionReply,
    ) -> Result<(), NativeRuntimeError>;
    /// `Session::abort()` — halts the current agent turn.
    fn abort(&mut self) -> Result<(), NativeRuntimeError>;
}

/// Starts Copilot sessions. The single point the real SDK plugs into.
pub trait CopilotTransport: Send + Sync {
    fn runtime_source(&self) -> CopilotRuntimeSource;
    /// Lists models via the SDK's model namespace.
    fn list_models(&self) -> Result<Vec<(String, String)>, NativeRuntimeError>;
    /// Starts a session, or returns the typed Copilot rejection that
    /// distinguishes "no seat" from "SSO" from "policy" from "quota".
    fn start_session(
        &self,
        request: &NativeTurnRequest,
        token: &CopilotAccessToken,
        policy: &CopilotSessionPolicy,
    ) -> Result<Box<dyn CopilotSession>, CopilotStartError>;
}

/// Session start failed either at the transport or inside Copilot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotStartError {
    /// The runtime itself never came up (spawn, handshake, protocol version).
    Runtime(NativeRuntimeError),
    /// Copilot answered and refused. Carries the entitlement signal.
    Rejected(CopilotSessionRejection),
}

impl From<CopilotStartError> for NativeRuntimeError {
    fn from(error: CopilotStartError) -> Self {
        match error {
            CopilotStartError::Runtime(error) => error,
            CopilotStartError::Rejected(rejection) => NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                format!("copilot refused the session: {}", rejection.code),
                false,
            ),
        }
    }
}

/// The transport used until `github-copilot-sdk` is linked in `Cargo.toml`.
///
/// It fails closed. Plan 037 explicitly forbids substituting a direct HTTP call
/// to Copilot, so the honest behaviour with no SDK linked is
/// `provider_unavailable` — not a fabricated agent.
pub struct UnlinkedSdkTransport;

/// Pinned CLI version this slice is written against, matching the crate's
/// `cli-version.txt` at the version recorded in the module docs.
pub const PINNED_SDK_VERSION: &str = "1.0.11";

const UNLINKED_MESSAGE: &str =
    "the GitHub Copilot SDK runtime is not linked into this Alfred build";

impl CopilotTransport for UnlinkedSdkTransport {
    fn runtime_source(&self) -> CopilotRuntimeSource {
        CopilotRuntimeSource::Unavailable {
            expected_version: PINNED_SDK_VERSION.into(),
        }
    }

    fn list_models(&self) -> Result<Vec<(String, String)>, NativeRuntimeError> {
        Err(unavailable())
    }

    fn start_session(
        &self,
        _request: &NativeTurnRequest,
        _token: &CopilotAccessToken,
        _policy: &CopilotSessionPolicy,
    ) -> Result<Box<dyn CopilotSession>, CopilotStartError> {
        Err(CopilotStartError::Runtime(unavailable()))
    }
}

fn unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::ProviderUnavailable, UNLINKED_MESSAGE, false)
}
