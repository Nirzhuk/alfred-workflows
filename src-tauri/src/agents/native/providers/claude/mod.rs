//! Alfred-native Claude products.
//!
//! The existing runtime in this file remains the separate `claude_api`
//! product: it is **not** Claude Code and is not subscription billed. The
//! `subscription` module adds a fail-closed managed product around the exact,
//! unmodified Claude Code 2.1.246 terminal binary. Neither product can fall
//! back to the other or to a user-installed CLI.
//!
//! Deliberately absent, and why:
//!
//! - The direct API runtime has no Claude.ai / Claude Code subscription login.
//!   The Agent SDK overview states: "Unless previously approved, Anthropic does
//!   not allow third party developers to offer claude.ai login or rate limits
//!   for their products."
//!   Alfred has no custom-renderer approval on record. Managed interactive
//!   login, when commercially permitted, stays entirely inside the unmodified
//!   publisher PTY.
//! - No `~/.claude/.credentials.json`, keychain scrape, `CLAUDE_CODE_OAUTH_TOKEN`,
//!   or `claude` subprocess. The runtime never looks at CLI state.
//! - No Agent SDK bridge. The SDK ships for Python and TypeScript only; a
//!   bundled Node/Python runtime's packaging and redistribution are unresolved,
//!   so the direct Rust client is the runtime boundary.
//!
//! See `plans/034-claude-native-harness.md` for the recorded provider decision.

mod auth;
mod package;
mod status;
mod subscription;
mod terminal;
mod transport;
mod wire;

pub use auth::*;
pub use package::*;
pub use status::*;
pub use subscription::*;
pub use terminal::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod subscription_tests;

use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agents::native::{
    NativeAgentRuntime, NativeCancellation, NativeCapabilities, NativeContentClass,
    NativeErrorCode, NativeEvent, NativeEventKind, NativeModel, NativeRuntimeDescriptor,
    NativeRuntimeError, NativeRuntimeRegistry, NativeToolExecutionOwner, NativeTurnHost,
    NativeTurnOutcome, NativeTurnRequest, NativeUsageSnapshot, ResolvedNativeAccount,
    NATIVE_EVENT_CONTRACT_VERSION, NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::AgentProvider;
use serde_json::Value;
use transport::ClaudeTransport;
use wire::{
    alfred_tool_request, build_request_body, initial_messages, parse_model_catalog,
    tool_result_block, validate_api_key, SseDecoder, StreamAccumulator, StreamSignal,
    MAX_TOOL_ITERATIONS,
};

/// Not "Claude Code": Anthropic's branding guidance forbids a third-party
/// product presenting itself as Claude Code.
pub const RUNTIME_ID: &str = "claude-native-anthropic-api";
pub const RUNTIME_VERSION: &str = "0.1.0";
pub const ACCOUNT_INTAKE_BLOCKED_CODE: &str = "claude_api_key_account_intake_unavailable";
pub const LIVE_SMOKE_BLOCKED_CODE: &str = "claude_live_api_key_smoke_missing";

pub(crate) struct ClaudeNativeRuntime {
    transport: Box<dyn ClaudeTransport>,
}

impl ClaudeNativeRuntime {
    #[cfg(test)]
    pub(super) fn new(transport: Box<dyn ClaudeTransport>) -> Self {
        Self { transport }
    }
}

/// Production registration is structurally blocked. The fixture constructor is
/// compiled only for tests, so callers cannot bypass these release gates by
/// directly constructing the HTTP runtime.
pub fn register(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!("{ACCOUNT_INTAKE_BLOCKED_CODE}; {LIVE_SMOKE_BLOCKED_CODE}"),
        false,
    ))
}

fn capabilities() -> NativeCapabilities {
    NativeCapabilities {
        // API key only. OAuth stays false until Anthropic approves a
        // third-party subscription integration.
        supports_api_key: true,
        supports_model_list: true,
        // The Messages API reports per-response token counts, but there is no
        // documented account-usage endpoint for a plain API key, so Alfred
        // reports "usage unavailable" rather than inferring a quota.
        supports_usage: false,
        supports_tool_calls: true,
        supports_approval_events: true,
        supports_native_filesystem: true,
        supports_native_shell: true,
        supports_patch: true,
        // Sessions/resume: the Messages API is stateless and Alfred replays its
        // own context, so no provider-side session is claimed.
        ..NativeCapabilities::default()
    }
}

fn api_key(account: &ResolvedNativeAccount) -> Result<String, NativeRuntimeError> {
    #[cfg(test)]
    if let Some(key) = account.credential.downcast_ref::<tests::TestApiKey>() {
        validate_api_key(&key.0)?;
        return Ok(key.0.clone());
    }
    let credential = account
        .credential
        .downcast_ref::<NativeAgentCredential>()
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "native Claude account did not resolve to an Alfred-managed credential",
                false,
            )
        })?;
    let key = credential.access_token().ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "native Claude account holds no Anthropic API key",
            false,
        )
    })?;
    validate_api_key(key)?;
    Ok(key.to_string())
}

impl NativeAgentRuntime for ClaudeNativeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: RUNTIME_ID.into(),
            runtime_version: RUNTIME_VERSION.into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: AgentProvider::ClaudeCode,
            product: crate::agent_accounts::models::AgentProductId::ClaudeApi,
            tool_execution_owner: NativeToolExecutionOwner::AlfredExecuted,
            capabilities: capabilities(),
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider != AgentProvider::ClaudeCode {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "native Claude runtime received another provider's account",
                false,
            ));
        }
        api_key(account).map(|_| ())
    }

    fn discover_models(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        let key = api_key(account)?;
        let body = self.transport.list_models(&key)?;
        parse_model_catalog(&body)
    }

    fn run_turn(
        &self,
        account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        let key = api_key(account)?;
        host.cancellation().checkpoint()?;
        host.emit(NativeEvent::new(0, NativeEventKind::TurnStarted))?;

        let mut messages = initial_messages(request);
        for _ in 0..MAX_TOOL_ITERATIONS {
            let body = build_request_body(request, &messages);
            let accumulator = self.stream_once(&key, &body, request, host)?;
            if accumulator.tool_calls().is_empty() {
                host.emit(NativeEvent::new(0, NativeEventKind::TurnCompleted))?;
                return Ok(NativeTurnOutcome { session_id: None });
            }
            messages.push(accumulator.assistant_message());
            let mut results = Vec::new();
            for call in accumulator.tool_calls() {
                host.cancellation().checkpoint()?;
                let tool_request = alfred_tool_request(call)?;
                let result = host.invoke_tool(tool_request)?;
                results.push(tool_result_block(&result));
            }
            // Every tool result for one assistant turn goes back in a single
            // user message, as the API requires for parallel tool use.
            messages.push(serde_json::json!({"role": "user", "content": results}));
        }
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "native turn exceeded its tool iteration bound",
            false,
        ))
    }

    fn cancel(&self, cancellation: &NativeCancellation) -> Result<(), NativeRuntimeError> {
        // Cooperative: the stream loop checkpoints between chunks and tool
        // calls, so the in-flight turn stops at its next boundary.
        cancellation.cancel();
        Ok(())
    }

    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        Ok(NativeUsageSnapshot::unavailable())
    }
}

impl ClaudeNativeRuntime {
    /// Streams one Messages API response, emitting bounded assistant deltas.
    fn stream_once(
        &self,
        key: &str,
        body: &Value,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<StreamAccumulator, NativeRuntimeError> {
        host.cancellation().checkpoint()?;
        let mut stream = self
            .transport
            .stream_messages(key, body, host.cancellation())?;
        let mut decoder = SseDecoder::default();
        let mut accumulator = StreamAccumulator::new(request.event_limits.max_text_bytes);
        let mut text = String::new();
        loop {
            host.cancellation().checkpoint()?;
            let Some(chunk) = stream.next_chunk(host.cancellation())? else {
                break;
            };
            for event in decoder.push(&chunk)? {
                for signal in accumulator.accept(&event)? {
                    match signal {
                        StreamSignal::Text(delta) => {
                            text.push_str(&delta);
                            let mut event = NativeEvent::new(0, NativeEventKind::AssistantDelta);
                            event.content_class = Some(NativeContentClass::Assistant);
                            event.text = Some(delta);
                            host.emit(event)?;
                        }
                    }
                }
            }
            if accumulator.completed() {
                break;
            }
        }
        if !accumulator.completed() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "Anthropic response stream ended before the turn completed",
                true,
            ));
        }
        accumulator.finish_text(&text);
        Ok(accumulator)
    }
}
