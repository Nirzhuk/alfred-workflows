//! Copilot SDK JSON-RPC session events → Plan 032 [`NativeEvent`].
//!
//! The SDK yields `SessionEvent { event_type: String, data: Map }`. That map is
//! **untrusted**: it comes from a child process over stdio. Everything here is
//! bounded, allow-listed, and redacted before a value reaches the shared
//! normalizer, which then applies the harness-wide limits again.
//!
//! Two Copilot-specific hardening steps happen before the shared normalizer:
//!
//! 1. `assistant.reasoning_delta` is dropped, not forwarded — the native event
//!    contract prohibits reasoning content outright.
//! 2. `gho_`, `ghu_`, and `github_pat_` token prefixes are redacted. Keeping
//!    all Copilot token classes explicit here makes the provider boundary
//!    classes Copilot's own auth issues.

use crate::agents::native::{
    redact_text, NativeContentClass, NativeErrorCode, NativeEvent, NativeEventKind,
    NativeRuntimeError,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;

/// One raw event off the SDK subscription.
#[derive(Clone, PartialEq)]
pub struct CopilotSdkEvent {
    pub event_type: String,
    pub data: Map<String, Value>,
}

impl fmt::Debug for CopilotSdkEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let event_type = if contains_provider_secret(&self.event_type) {
            "[REDACTED]".to_string()
        } else {
            scrub(&self.event_type)
        };
        formatter
            .debug_struct("CopilotSdkEvent")
            .field("event_type", &event_type)
            .field("data", &"[REDACTED]")
            .finish()
    }
}

/// An SDK event type longer than this is not one GitHub emits.
const MAX_EVENT_TYPE_BYTES: usize = 128;
/// Identifier fields (session/turn/tool/approval ids) are opaque and short.
const MAX_ID_BYTES: usize = 128;
/// Secret-bearing provider identifiers are replaced with a stable, bounded
/// digest. Sixteen bytes retain ample collision resistance while keeping the
/// result below the shared identifier limit.
const OPAQUE_ID_DIGEST_BYTES: usize = 16;
/// A single delta is bounded well under the harness text limit so one hostile
/// frame cannot consume the whole turn budget.
const MAX_DELTA_BYTES: usize = 16 * 1024;
/// Tool output per frame; the shared normalizer bounds the aggregate again.
const MAX_TOOL_OUTPUT_BYTES: usize = 32 * 1024;

/// Copilot token prefixes rejected or scrubbed at provider boundaries.
const COPILOT_TOKEN_PREFIXES: [&str; 3] = ["gho_", "ghu_", "github_pat_"];
/// Shared secret forms matched without prose-boundary assumptions because a
/// provider identifier may embed them after an otherwise benign prefix.
const SHARED_IDENTIFIER_SECRET_FRAGMENTS: [&str; 9] = [
    "sk-",
    "ghp_",
    "xox",
    "bearer ",
    "basic ",
    "cookie:",
    "set-cookie:",
    "authorization:",
    "credential_path=",
];

/// What the mapper decided to do with one SDK event.
#[derive(Debug, Clone, PartialEq)]
pub enum MappedEvent {
    /// Forward this to the host.
    Emit(NativeEvent),
    /// Deliberately dropped (reasoning, heartbeat, or an unmodelled type).
    Drop,
}

/// Assigns strictly increasing sequences and maps SDK events.
pub struct CopilotEventMapper {
    next_sequence: u32,
    session_id: Option<String>,
    turn_id: Option<String>,
}

impl CopilotEventMapper {
    pub fn new() -> Self {
        Self {
            next_sequence: 0,
            session_id: None,
            turn_id: None,
        }
    }

    fn take_sequence(&mut self) -> Result<u32, NativeRuntimeError> {
        // Overflow here would break the normalizer's strictly-increasing rule,
        // so fail the turn instead of wrapping.
        let sequence = self.next_sequence;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("copilot event sequence overflowed"))?;
        Ok(sequence)
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Maps one raw SDK event.
    ///
    /// Returns `Err` only when the frame is structurally hostile (oversized or
    /// malformed in a way that must fail the turn); an event type Alfred does
    /// not model is dropped instead, so a Copilot release that adds a frame
    /// type cannot break a running turn.
    pub fn map(&mut self, event: &CopilotSdkEvent) -> Result<MappedEvent, NativeRuntimeError> {
        if event.event_type.len() > MAX_EVENT_TYPE_BYTES {
            return Err(invalid("copilot event type exceeds the supported length"));
        }

        // Reasoning is prohibited by the native event contract; never forward
        // it, and never fall through to a generic text mapping.
        if event.event_type.ends_with("reasoning_delta") || event.event_type.ends_with("reasoning")
        {
            return Ok(MappedEvent::Drop);
        }

        match event.event_type.as_str() {
            "session.start" | "session.created" | "session.started" => {
                self.session_id = bounded_id(&event.data, "sessionId")?;
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::SessionStarted);
                native.session_id = self.session_id.clone();
                Ok(MappedEvent::Emit(native))
            }
            "assistant.turn_start" | "turn.started" | "assistant.turn_started" => {
                self.turn_id = bounded_id(&event.data, "turnId")?;
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::TurnStarted);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                Ok(MappedEvent::Emit(native))
            }
            "assistant.message_delta" | "assistant.message" => {
                let Some(text) = bounded_text(
                    &event.data,
                    &["deltaContent", "delta", "text", "content"],
                    MAX_DELTA_BYTES,
                )?
                else {
                    return Ok(MappedEvent::Drop);
                };
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::AssistantDelta);
                native.content_class = Some(NativeContentClass::Assistant);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.text = Some(scrub(&text));
                Ok(MappedEvent::Emit(native))
            }
            "tool.execution_start" | "tool.started" | "tool.invocation_started" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::ToolStarted);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.tool_call_id = bounded_id(&event.data, "toolCallId")?;
                native.tool_name =
                    bounded_text(&event.data, &["name", "toolName"], 128)?.map(|name| scrub(&name));
                Ok(MappedEvent::Emit(native))
            }
            "tool.execution_progress" | "tool.progress" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::ToolProgress);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.tool_call_id = bounded_id(&event.data, "toolCallId")?;
                native.text = bounded_text(
                    &event.data,
                    &["progressMessage", "message", "text"],
                    MAX_DELTA_BYTES,
                )?
                .map(|text| scrub(&text));
                Ok(MappedEvent::Emit(native))
            }
            "tool.execution_complete" | "tool.completed" | "tool.invocation_completed" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::ToolCompleted);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.tool_call_id = bounded_id(&event.data, "toolCallId")?;
                // Current `tool.execution_complete.result` is a structured
                // content union, not plain text. Alfred custom tools already
                // emitted their bounded output through `NativeTurnHost`, so
                // never serialize that unbounded SDK structure a second time.
                native.tool_output = if event.event_type == "tool.execution_complete" {
                    None
                } else {
                    bounded_text(&event.data, &["output", "result"], MAX_TOOL_OUTPUT_BYTES)?
                        .map(|output| scrub(&output))
                };
                Ok(MappedEvent::Emit(native))
            }
            "permission.requested" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::ApprovalRequested);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.approval_id = bounded_id(&event.data, "requestId")?;
                Ok(MappedEvent::Emit(native))
            }
            "permission.completed" | "permission.resolved" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::ApprovalResolved);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.approval_id = bounded_id(&event.data, "requestId")?;
                native.approved =
                    event
                        .data
                        .get("approved")
                        .and_then(Value::as_bool)
                        .or_else(|| {
                            let kind = event
                                .data
                                .get("result")
                                .and_then(Value::as_object)
                                .and_then(|result| result.get("kind"))
                                .and_then(Value::as_str)?;
                            if kind.starts_with("approved") || kind.starts_with("approve") {
                                Some(true)
                            } else if kind.starts_with("rejected")
                                || kind.starts_with("reject")
                                || kind.starts_with("denied")
                            {
                                Some(false)
                            } else {
                                None
                            }
                        });
                Ok(MappedEvent::Emit(native))
            }
            "assistant.turn_end"
            | "session.idle"
            | "turn.completed"
            | "assistant.turn_completed" => {
                if event.event_type == "session.idle"
                    && event.data.get("aborted").and_then(Value::as_bool) == Some(true)
                {
                    let sequence = self.take_sequence()?;
                    let mut native = NativeEvent::new(sequence, NativeEventKind::TurnCancelled);
                    native.session_id = self.session_id.clone();
                    native.turn_id = self.turn_id.clone();
                    return Ok(MappedEvent::Emit(native));
                }
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::TurnCompleted);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                Ok(MappedEvent::Emit(native))
            }
            "abort" | "agent.interrupted" | "turn.aborted" | "session.aborted" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::TurnCancelled);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                Ok(MappedEvent::Emit(native))
            }
            "turn.failed" | "session.error" => {
                let sequence = self.take_sequence()?;
                let mut native = NativeEvent::new(sequence, NativeEventKind::TurnFailed);
                native.session_id = self.session_id.clone();
                native.turn_id = self.turn_id.clone();
                native.error = bounded_text(&event.data, &["message", "error"], 4 * 1024)?
                    .map(|error| scrub(&error));
                Ok(MappedEvent::Emit(native))
            }
            _ => Ok(MappedEvent::Drop),
        }
    }
}

impl Default for CopilotEventMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared redaction plus explicit Copilot token-class coverage.
pub fn scrub(value: &str) -> String {
    redact_copilot_tokens(&redact_text(value))
}

/// Replaces Copilot token-shaped runs with `[REDACTED]`.
fn redact_copilot_tokens(value: &str) -> String {
    let mut spans = Vec::new();
    for prefix in COPILOT_TOKEN_PREFIXES {
        let mut search = 0usize;
        while let Some(offset) = value[search..].find(prefix) {
            let start = search + offset;
            let at_boundary = start == 0
                || value[..start].chars().next_back().is_some_and(|character| {
                    character.is_whitespace()
                        || matches!(
                            character,
                            '"' | '\''
                                | ','
                                | ';'
                                | ':'
                                | '('
                                | ')'
                                | '{'
                                | '}'
                                | '['
                                | ']'
                                | '='
                                | '<'
                                | '>'
                        )
                });
            let end = value[start..]
                .find(|character: char| {
                    character.is_whitespace()
                        || matches!(character, ',' | ';' | '"' | '\'' | ')' | '}')
                })
                .map(|end| start + end)
                .unwrap_or(value.len());
            if at_boundary && end > start {
                spans.push((start, end));
            }
            search = start + prefix.len();
        }
    }
    if spans.is_empty() {
        return value.to_string();
    }
    spans.sort_by_key(|(start, _)| *start);
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    for (start, end) in spans {
        if start < cursor {
            continue;
        }
        output.push_str(&value[cursor..start]);
        output.push_str("[REDACTED]");
        cursor = end;
    }
    output.push_str(&value[cursor..]);
    output
}

fn bounded_id(data: &Map<String, Value>, key: &str) -> Result<Option<String>, NativeRuntimeError> {
    let Some(value) = data.get(key) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| invalid("copilot event identifier is not a string"))?
        .trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.len() > MAX_ID_BYTES {
        return Err(invalid(
            "copilot event identifier exceeds the supported length",
        ));
    }
    if contains_provider_secret(text) {
        return Ok(Some(opaque_provider_id(key, text)));
    }
    Ok(Some(text.to_string()))
}

/// Provider identifiers are not display text: replacing a token with a visible
/// redaction marker would break lifecycle correlation and is not a valid native
/// identifier. Hash the full raw value with a field-specific domain instead.
fn opaque_provider_id(field: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"alfred/copilot/provider-id/v1\0");
    hasher.update(field.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let suffix = digest
        .iter()
        .take(OPAQUE_ID_DIGEST_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("copilot_opaque_{suffix}")
}

/// Identifies both provider-specific OAuth token classes and every shared
/// secret marker. Copilot prefixes are matched anywhere and case-insensitively
/// because an identifier has no prose boundary on which redaction can rely.
pub(super) fn contains_provider_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    COPILOT_TOKEN_PREFIXES
        .iter()
        .any(|prefix| lower.contains(prefix))
        || SHARED_IDENTIFIER_SECRET_FRAGMENTS
            .iter()
            .any(|fragment| lower.contains(fragment))
        || scrub(value) != value
}

/// Reads the first present key, refusing anything over `max_bytes` rather than
/// truncating — a frame that large is a protocol violation, not long output.
fn bounded_text(
    data: &Map<String, Value>,
    keys: &[&str],
    max_bytes: usize,
) -> Result<Option<String>, NativeRuntimeError> {
    for key in keys {
        let Some(value) = data.get(*key) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            return Err(invalid("copilot event text field is not a string"));
        };
        if text.len() > max_bytes {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::EventLimitExceeded,
                "copilot event field exceeds the supported size",
                false,
            ));
        }
        if text.is_empty() {
            return Ok(None);
        }
        return Ok(Some(text.to_string()));
    }
    Ok(None)
}

fn invalid(message: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidEvent, message, false)
}
