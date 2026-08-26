//! Shared-contract adapter and bounded tool loop for native Gemini.

use super::credential::{credential_from, GeminiCredential};
use super::protocol::{
    alfred_tool_request, blocked_error, build_generate_request, enforce_chunk_budget,
    function_result, parse_models, parse_stream_chunk, GeminiChunkEvent, GeminiFunctionCall,
    GeminiHistoryEntry, GeminiSseDecoder, MAX_FUNCTION_CALLS_PER_ROUND, MAX_TOOL_ROUNDS,
};
use super::transport::GeminiTransport;
use super::{GEMINI_NATIVE_RUNTIME_ID, GEMINI_NATIVE_RUNTIME_VERSION};
use crate::agents::native::{
    redact_text, CapabilityReportStatus, NativeAgentRuntime, NativeCancellation,
    NativeCapabilities, NativeContentClass, NativeErrorCode, NativeEvent, NativeEventKind,
    NativeModel, NativeRuntimeDescriptor, NativeRuntimeError, NativeRuntimeRegistry,
    NativeToolExecutionOwner, NativeTurnHost, NativeTurnOutcome, NativeTurnRequest,
    NativeUsageSnapshot, ResolvedNativeAccount, NATIVE_EVENT_CONTRACT_VERSION,
    NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::AgentProvider;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

pub const ACCOUNT_INTAKE_BLOCKED_CODE: &str = "gemini_api_key_account_intake_unavailable";
pub const LIVE_SMOKE_BLOCKED_CODE: &str = "gemini_live_api_key_smoke_missing";

pub(super) struct GeminiNativeRuntime {
    transport: Arc<dyn GeminiTransport>,
}

impl GeminiNativeRuntime {
    #[cfg(test)]
    pub(super) fn new(transport: Arc<dyn GeminiTransport>) -> Self {
        Self { transport }
    }

    fn stream_round(
        &self,
        credential: &GeminiCredential,
        request: &NativeTurnRequest,
        history: &[GeminiHistoryEntry],
        host: &mut dyn NativeTurnHost,
    ) -> Result<GeminiRound, NativeRuntimeError> {
        host.cancellation().checkpoint()?;
        let body = build_generate_request(request, history);
        let mut stream = self.transport.stream_generate(
            credential,
            &request.model,
            &body,
            host.cancellation(),
        )?;
        let mut decoder = GeminiSseDecoder::default();
        let mut round = GeminiRound::default();
        let mut chunks = 0usize;

        loop {
            host.cancellation().checkpoint()?;
            let Some(bytes) = stream.next_chunk(host.cancellation())? else {
                break;
            };
            for payload in decoder.push(&bytes)? {
                chunks = chunks.saturating_add(1);
                enforce_chunk_budget(chunks)?;
                for event in parse_stream_chunk(&payload, &request.event_limits)? {
                    match event {
                        GeminiChunkEvent::Text(text) => {
                            let text = redact_text(&credential.redact(&text));
                            if !text.is_empty() {
                                let mut native =
                                    NativeEvent::new(0, NativeEventKind::AssistantDelta);
                                native.content_class = Some(NativeContentClass::Assistant);
                                native.text = Some(text.clone());
                                host.emit(native)?;
                                round.text_parts.push(text);
                            }
                        }
                        GeminiChunkEvent::FunctionCall(call) => {
                            round.calls.push(call);
                            if round.calls.len() > MAX_FUNCTION_CALLS_PER_ROUND {
                                return Err(NativeRuntimeError::new(
                                    NativeErrorCode::EventLimitExceeded,
                                    "gemini returned too many function calls in one round",
                                    false,
                                ));
                            }
                        }
                        GeminiChunkEvent::Blocked { reason } => {
                            return Err(blocked_error(&reason));
                        }
                        GeminiChunkEvent::Usage {
                            input_tokens,
                            output_tokens,
                        } => {
                            round.usage = Some((input_tokens, output_tokens));
                        }
                        GeminiChunkEvent::Finished { reason } => {
                            if round.finish_reason.replace(reason).is_some() {
                                return Err(invalid_event(
                                    "gemini stream carried more than one finish reason",
                                ));
                            }
                        }
                    }
                }
            }
        }
        decoder.finish()?;
        let reason = round.finish_reason.as_deref().ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "gemini response stream ended before a finish reason",
                true,
            )
        })?;
        match reason {
            "stop" => {}
            "max_tokens" if round.calls.is_empty() => {
                let mut warning = NativeEvent::new(0, NativeEventKind::Warning);
                warning.text = Some("Gemini stopped at the model output limit.".into());
                host.emit(warning)?;
            }
            "max_tokens" => {
                return Err(invalid_event(
                    "gemini truncated a response that contained a function call",
                ));
            }
            "malformed_function_call" | "unexpected_tool_call" | "too_many_tool_calls" => {
                return Err(invalid_event(
                    "gemini could not produce a valid bounded tool call",
                ));
            }
            _ => {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::ProviderUnavailable,
                    format!("gemini ended the turn with unsupported reason {reason}"),
                    false,
                ));
            }
        }
        if round.text_parts.is_empty() && round.calls.is_empty() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "gemini completed without text or a function call",
                false,
            ));
        }
        Ok(round)
    }
}

impl NativeAgentRuntime for GeminiNativeRuntime {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: GEMINI_NATIVE_RUNTIME_ID.into(),
            runtime_version: GEMINI_NATIVE_RUNTIME_VERSION.into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: AgentProvider::Gemini,
            product: crate::agent_accounts::models::AgentProductId::GeminiApi,
            tool_execution_owner: NativeToolExecutionOwner::AlfredExecuted,
            capabilities: NativeCapabilities {
                supports_api_key: true,
                supports_model_list: true,
                // Response token counts do not expose remaining account quota.
                supports_usage: false,
                supports_tool_calls: true,
                supports_approval_events: true,
                // Execution stays behind Alfred's shared tool boundary.
                supports_native_filesystem: true,
                supports_native_shell: true,
                supports_patch: true,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(&self, account: &ResolvedNativeAccount) -> Result<(), NativeRuntimeError> {
        if account.provider != AgentProvider::Gemini {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "gemini native runtime received another provider's account",
                false,
            ));
        }
        credential_from(account).map(|_| ())
    }

    fn discover_models(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        let credential = credential_from(account)?;
        let body = self.transport.list_models(&credential)?;
        parse_models(&body)
    }

    fn run_turn(
        &self,
        account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        let credential = credential_from(account)?;
        host.cancellation().checkpoint()?;
        host.emit(NativeEvent::new(0, NativeEventKind::TurnStarted))?;
        let mut history = Vec::new();

        // One final no-tool response is allowed after the maximum tool round.
        for round_index in 0..=MAX_TOOL_ROUNDS {
            let round = self.stream_round(&credential, request, &history, host)?;
            if round.calls.is_empty() {
                let mut completed = NativeEvent::new(0, NativeEventKind::TurnCompleted);
                completed
                    .metadata
                    .insert("accountUsageState".into(), json!("unavailable"));
                if let Some((input, output)) = round.usage {
                    // Per-turn token metadata is useful, but it is not an
                    // account quota reading and does not change Usage state.
                    // Avoid secret-shaped metadata keys (`*token*`); the
                    // shared normalizer correctly redacts those by design.
                    completed
                        .metadata
                        .insert("providerInputUnitCount".into(), json!(input));
                    completed
                        .metadata
                        .insert("providerOutputUnitCount".into(), json!(output));
                }
                host.emit(completed)?;
                return Ok(NativeTurnOutcome { session_id: None });
            }

            if round_index == MAX_TOOL_ROUNDS {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::InvalidRequest,
                    "gemini turn exceeded its bounded function-call rounds",
                    false,
                ));
            }

            history.push(GeminiHistoryEntry::ModelResponse {
                text_parts: round.text_parts,
                calls: round.calls.clone(),
            });
            let mut results = Vec::with_capacity(round.calls.len());
            for (call_index, call) in round.calls.iter().enumerate() {
                host.cancellation().checkpoint()?;
                reject_credential_in_call(&credential, call)?;
                let fallback_id = format!("gemini_{round_index}_{call_index}");
                let tool_request = alfred_tool_request(call, fallback_id)?;
                let result = host.invoke_tool(tool_request)?;
                let mut provider_result = function_result(call, &result);
                provider_result.output = redact_text(&credential.redact(&provider_result.output));
                results.push(provider_result);
            }
            history.push(GeminiHistoryEntry::ToolResults(results));
        }

        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "gemini turn exceeded its bounded function-call rounds",
            false,
        ))
    }

    fn cancel(&self, cancellation: &NativeCancellation) -> Result<(), NativeRuntimeError> {
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

#[derive(Default)]
struct GeminiRound {
    text_parts: Vec<String>,
    calls: Vec<GeminiFunctionCall>,
    usage: Option<(u64, u64)>,
    finish_reason: Option<String>,
}

fn reject_credential_in_call(
    credential: &GeminiCredential,
    call: &GeminiFunctionCall,
) -> Result<(), NativeRuntimeError> {
    let encoded = serde_json::to_string(&call.args).map_err(|_| {
        NativeRuntimeError::new(
            NativeErrorCode::InvalidEvent,
            "gemini function arguments could not be encoded",
            false,
        )
    })?;
    if credential.redact(&encoded) != encoded {
        Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "gemini attempted to place the account credential in a tool call",
            false,
        ))
    } else {
        Ok(())
    }
}

fn invalid_event(message: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidEvent, message, false)
}

/// Production registration stays fail-closed until Settings can create this
/// exact API-key account type and a live paid-project smoke has passed.
pub fn register(_registry: &NativeRuntimeRegistry) -> Result<(), NativeRuntimeError> {
    Err(NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        format!("{ACCOUNT_INTAKE_BLOCKED_CODE}; {LIVE_SMOKE_BLOCKED_CODE}"),
        false,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiNativeGate {
    pub gate: &'static str,
    pub status: CapabilityReportStatus,
    pub evidence: &'static str,
}

pub fn native_gates() -> Vec<GeminiNativeGate> {
    vec![
        GeminiNativeGate {
            gate: "official_api_surface",
            status: CapabilityReportStatus::Supported,
            evidence: "fixed-host Gemini Developer API v1beta with x-goog-api-key",
        },
        GeminiNativeGate {
            gate: "account_intake",
            status: CapabilityReportStatus::Blocked,
            evidence: ACCOUNT_INTAKE_BLOCKED_CODE,
        },
        GeminiNativeGate {
            gate: "bounded_transport_tools_cancellation_redaction",
            status: CapabilityReportStatus::Supported,
            evidence: "scripted native registry fixtures cover stream, tools, cancellation, and credential redaction",
        },
        GeminiNativeGate {
            gate: "runtime_package",
            status: CapabilityReportStatus::Supported,
            evidence: "direct HTTPS uses existing packaged Rust dependencies; no provider binary or CLI is packaged",
        },
        GeminiNativeGate {
            gate: "live_paid_project_smoke",
            status: CapabilityReportStatus::Blocked,
            evidence: LIVE_SMOKE_BLOCKED_CODE,
        },
    ]
}

pub fn native_ready() -> bool {
    native_gates()
        .iter()
        .all(|entry| entry.status != CapabilityReportStatus::Blocked)
}
