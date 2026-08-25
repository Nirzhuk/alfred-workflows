//! Pure protocol layer for the Anthropic Messages API.
//!
//! Nothing here performs I/O, so every request shape, stream decode, and error
//! classification is exercised by fixtures. Official surfaces only:
//! `POST /v1/messages` and `GET /v1/models` with the `x-api-key` and
//! `anthropic-version` headers documented at
//! <https://platform.claude.com/docs/en/manage-claude/authentication>.

use crate::agents::native::{
    redact_text, AlfredToolKind, AlfredToolRequest, AlfredToolResult, AlfredToolStatus,
    NativeContextRole, NativeErrorCode, NativeRuntimeError, NativeToolCapabilitySet,
    NativeTurnRequest, TOOL_CONTRACT_VERSION,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=100";

/// Bounded output ceiling for one native turn. The harness caps assistant text
/// separately; this stops a runaway generation before that limit is reached.
pub const MAX_OUTPUT_TOKENS: u32 = 8_192;
/// A turn may hand control back to Alfred tools this many times before the
/// runtime gives up. Deterministic, so a looping model cannot run forever.
pub const MAX_TOOL_ITERATIONS: usize = 8;
pub const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
pub const TOOL_TIMEOUT_MS: u64 = 60_000;
/// Guards the SSE reassembly buffer against an unterminated event.
pub const MAX_SSE_BUFFER_BYTES: usize = 1024 * 1024;
pub const MAX_SSE_EVENTS: usize = 20_000;
pub const MAX_MODEL_CATALOG_ENTRIES: usize = 512;
pub const MAX_MODEL_ID_BYTES: usize = 256;
pub const MAX_MODEL_LABEL_BYTES: usize = 256;
/// Anthropic API keys are the only credential this runtime accepts.
pub const API_KEY_PREFIX: &str = "sk-ant-api";

/// Stable classification of a provider failure, independent of the wording the
/// API happens to return. Fixtures assert on this, not on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeFailure {
    InvalidAuth,
    Billing,
    PermissionDenied,
    ModelUnavailable,
    ContextLimit,
    RequestTooLarge,
    InvalidRequest,
    RateLimited,
    Overloaded,
    ProviderUnavailable,
}

impl ClaudeFailure {
    pub fn code(self) -> NativeErrorCode {
        match self {
            Self::InvalidAuth | Self::Billing => NativeErrorCode::AccountUnavailable,
            Self::PermissionDenied => NativeErrorCode::PermissionDenied,
            Self::ModelUnavailable => NativeErrorCode::ModelUnavailable,
            Self::ContextLimit | Self::RequestTooLarge | Self::InvalidRequest => {
                NativeErrorCode::InvalidRequest
            }
            Self::RateLimited | Self::Overloaded | Self::ProviderUnavailable => {
                NativeErrorCode::ProviderUnavailable
            }
        }
    }

    pub fn retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::Overloaded | Self::ProviderUnavailable)
    }

    /// Alfred-owned copy. The provider's own message never reaches the user, so
    /// a leaked key or prompt echo cannot ride out on an error.
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidAuth => "Anthropic API key is invalid, revoked, or expired",
            Self::Billing => "Anthropic account billing needs attention",
            Self::PermissionDenied => "Anthropic API key lacks permission for this request",
            Self::ModelUnavailable => "selected model is unavailable for this API key",
            Self::ContextLimit => "native request exceeded the model context limit",
            Self::RequestTooLarge => "native request exceeded the Anthropic request size limit",
            Self::InvalidRequest => "Anthropic rejected the native request",
            Self::RateLimited => "Anthropic API rate limit reached",
            Self::Overloaded => "Anthropic API is temporarily overloaded",
            Self::ProviderUnavailable => "Anthropic API is unavailable",
        }
    }

    pub fn error(self) -> NativeRuntimeError {
        NativeRuntimeError::new(self.code(), self.message(), self.retryable())
    }
}

/// Maps an HTTP status plus the documented `error.type` onto a stable failure.
///
/// Status wins for the codes Anthropic documents unambiguously; `error.type`
/// only refines a 400 into the context-limit case.
pub fn classify_status(status: u16, body: &str) -> ClaudeFailure {
    let error_type = parse_error_type(body).unwrap_or_default();
    match status {
        400 => classify_bad_request(body, &error_type),
        401 => ClaudeFailure::InvalidAuth,
        402 => ClaudeFailure::Billing,
        403 => ClaudeFailure::PermissionDenied,
        404 => ClaudeFailure::ModelUnavailable,
        413 => ClaudeFailure::RequestTooLarge,
        429 => ClaudeFailure::RateLimited,
        529 => ClaudeFailure::Overloaded,
        status if (500..600).contains(&status) => ClaudeFailure::ProviderUnavailable,
        _ => ClaudeFailure::InvalidRequest,
    }
}

/// Classifies an SSE `error` event, which arrives after a 200 response.
pub fn classify_stream_error(event: &Value) -> ClaudeFailure {
    let error_type = event
        .get("error")
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = event
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match error_type {
        "authentication_error" => ClaudeFailure::InvalidAuth,
        "billing_error" => ClaudeFailure::Billing,
        "permission_error" => ClaudeFailure::PermissionDenied,
        "not_found_error" => ClaudeFailure::ModelUnavailable,
        "request_too_large" => ClaudeFailure::RequestTooLarge,
        "rate_limit_error" => ClaudeFailure::RateLimited,
        "overloaded_error" => ClaudeFailure::Overloaded,
        "api_error" | "timeout_error" => ClaudeFailure::ProviderUnavailable,
        "invalid_request_error" => classify_bad_request(message, error_type),
        _ => ClaudeFailure::ProviderUnavailable,
    }
}

fn classify_bad_request(body: &str, error_type: &str) -> ClaudeFailure {
    let lower = body.to_ascii_lowercase();
    let context_limit = lower.contains("context")
        || lower.contains("prompt is too long")
        || lower.contains("too many tokens")
        || lower.contains("max_tokens");
    if context_limit {
        ClaudeFailure::ContextLimit
    } else if error_type == "request_too_large" {
        ClaudeFailure::RequestTooLarge
    } else {
        ClaudeFailure::InvalidRequest
    }
}

fn parse_error_type(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("error")?
        .get("type")?
        .as_str()
        .map(str::to_string)
}

/// The Alfred tools this runtime is willing to expose, gated by the capability
/// set the request granted. No MCP or subagent tool is ever advertised.
pub fn tool_definitions(capabilities: &NativeToolCapabilitySet) -> Vec<Value> {
    let mut tools = Vec::new();
    if capabilities.filesystem {
        tools.push(tool_schema(
            "alfred_read_file",
            "Read a UTF-8 file inside the Alfred workspace.",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Workspace-relative or absolute path inside the workspace."}},
                "required": ["path"],
                "additionalProperties": false
            }),
        ));
        tools.push(tool_schema(
            "alfred_write_file",
            "Write UTF-8 contents to a file inside the Alfred workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "contents": {"type": "string"}
                },
                "required": ["path", "contents"],
                "additionalProperties": false
            }),
        ));
        tools.push(tool_schema(
            "alfred_edit_file",
            "Replace an exact string inside a workspace file.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "find": {"type": "string"},
                    "replace": {"type": "string"}
                },
                "required": ["path", "find", "replace"],
                "additionalProperties": false
            }),
        ));
        tools.push(tool_schema(
            "alfred_list_directory",
            "List the entries of a directory inside the Alfred workspace.",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }),
        ));
    }
    if capabilities.shell {
        tools.push(tool_schema(
            "alfred_run_command",
            "Run a command in the Alfred workspace. Alfred enforces its own permission profile.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Working directory inside the workspace."},
                    "command": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["path", "command"],
                "additionalProperties": false
            }),
        ));
    }
    if capabilities.patch {
        tools.push(tool_schema(
            "alfred_apply_patch",
            "Apply a unified diff to a file inside the Alfred workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "patch": {"type": "string"}
                },
                "required": ["path", "patch"],
                "additionalProperties": false
            }),
        ));
    }
    tools
}

fn tool_schema(name: &str, description: &str, input_schema: Value) -> Value {
    json!({"name": name, "description": description, "input_schema": input_schema})
}

fn tool_kind(name: &str) -> Option<AlfredToolKind> {
    match name {
        "alfred_read_file" => Some(AlfredToolKind::FileRead),
        "alfred_write_file" => Some(AlfredToolKind::FileWrite),
        "alfred_edit_file" => Some(AlfredToolKind::FileEdit),
        "alfred_list_directory" => Some(AlfredToolKind::DirectoryList),
        "alfred_run_command" => Some(AlfredToolKind::Shell),
        "alfred_apply_patch" => Some(AlfredToolKind::ApplyPatch),
        _ => None,
    }
}

/// Translates one `tool_use` block into an Alfred tool request. The Alfred
/// boundary re-validates everything; this only shapes the call.
pub fn alfred_tool_request(call: &ToolCall) -> Result<AlfredToolRequest, NativeRuntimeError> {
    let kind = tool_kind(&call.name).ok_or_else(|| {
        NativeRuntimeError::new(
            NativeErrorCode::CapabilityUnsupported,
            "the model requested a tool Alfred does not expose",
            false,
        )
    })?;
    let input: Map<String, Value> = serde_json::from_str::<Value>(&call.input_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "the model produced an unparsable tool input object",
                false,
            )
        })?;
    let mut request = AlfredToolRequest::new(call.id.clone(), kind, call.name.clone());
    request.timeout_ms = TOOL_TIMEOUT_MS;
    request.max_output_bytes = MAX_TOOL_OUTPUT_BYTES;
    let mut remaining = input.clone();
    if let Some(path) = remaining.remove("path").and_then(|value| match value {
        Value::String(path) => Some(path),
        _ => None,
    }) {
        request.path = Some(path.into());
    }
    if let Some(Value::Array(command)) = remaining.remove("command") {
        request.arguments = command
            .into_iter()
            .map(|value| match value {
                Value::String(argument) => Ok(argument),
                _ => Err(NativeRuntimeError::new(
                    NativeErrorCode::InvalidRequest,
                    "shell command arguments must be strings",
                    false,
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
    }
    request.input = remaining;
    Ok(request)
}

/// Encodes an Alfred tool outcome as a `tool_result` content block.
pub fn tool_result_block(result: &AlfredToolResult) -> Value {
    let is_error = !matches!(result.status, AlfredToolStatus::Completed);
    json!({
        "type": "tool_result",
        "tool_use_id": result.request_id,
        "is_error": is_error,
        "content": [{"type": "text", "text": result.output}],
    })
}

/// Builds the Messages API body for one iteration of the turn.
pub fn build_request_body(request: &NativeTurnRequest, messages: &[Value]) -> Value {
    let mut system = Vec::new();
    for block in &request.context {
        match block.role {
            NativeContextRole::System => system.push(json!({"type": "text", "text": block.content})),
            NativeContextRole::Skill => {
                let name = block.name.as_deref().unwrap_or("skill");
                system.push(json!({
                    "type": "text",
                    "text": format!("# Alfred skill: {name}\n\n{}", block.content),
                }));
            }
            _ => {}
        }
    }
    let mut body = json!({
        "model": request.model,
        "max_tokens": MAX_OUTPUT_TOKENS,
        "stream": true,
        "messages": messages,
    });
    if !system.is_empty() {
        body["system"] = Value::Array(system);
    }
    let tools = tool_definitions(&request.tool_capabilities);
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

/// The opening user turn, taken from the prepared native context.
pub fn initial_messages(request: &NativeTurnRequest) -> Vec<Value> {
    let mut messages = Vec::new();
    for block in &request.context {
        let role = match block.role {
            NativeContextRole::User => "user",
            NativeContextRole::Assistant => "assistant",
            _ => continue,
        };
        messages.push(json!({
            "role": role,
            "content": [{"type": "text", "text": block.content}],
        }));
    }
    messages
}

/// Reassembles `data:` payloads from an SSE byte stream.
#[derive(Default)]
pub struct SseDecoder {
    buffer: String,
    events: usize,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Value>, NativeRuntimeError> {
        let text = std::str::from_utf8(chunk).map_err(|_| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidEvent,
                "provider stream chunk was not valid UTF-8",
                false,
            )
        })?;
        self.buffer.push_str(text);
        if self.buffer.len() > MAX_SSE_BUFFER_BYTES {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::EventLimitExceeded,
                "provider stream event exceeded its byte limit",
                false,
            ));
        }
        let mut parsed = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let line = self.buffer[..index].trim_end_matches('\r').to_string();
            self.buffer.drain(..=index);
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            self.events = self.events.saturating_add(1);
            if self.events > MAX_SSE_EVENTS {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "provider stream exceeded its event limit",
                    false,
                ));
            }
            parsed.push(serde_json::from_str::<Value>(payload).map_err(|_| {
                NativeRuntimeError::new(
                    NativeErrorCode::InvalidEvent,
                    "provider stream event was not valid JSON",
                    false,
                )
            })?);
        }
        Ok(parsed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input_json: String,
}

/// What the accumulator wants the runtime to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamSignal {
    /// Assistant text ready to be emitted as an `assistant_delta`.
    Text(String),
}

#[derive(Debug, Default)]
enum BlockState {
    #[default]
    Ignored,
    Text,
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    /// Thinking blocks are accumulated verbatim so the assistant turn can be
    /// replayed to the API unchanged (the API rejects modified thinking
    /// blocks). They are never emitted, logged, or returned as output.
    Thinking {
        raw: Map<String, Value>,
    },
}

/// Turns decoded SSE events into assistant text, tool calls, and the raw
/// assistant content needed to continue a tool loop.
#[derive(Default)]
pub struct StreamAccumulator {
    blocks: BTreeMap<u64, BlockState>,
    content: Vec<Value>,
    tool_calls: Vec<ToolCall>,
    stop_reason: Option<String>,
    completed: bool,
    text_bytes: usize,
    max_text_bytes: usize,
}

impl StreamAccumulator {
    pub fn new(max_text_bytes: usize) -> Self {
        Self {
            max_text_bytes,
            ..Self::default()
        }
    }

    pub fn stop_reason(&self) -> Option<&str> {
        self.stop_reason.as_deref()
    }

    pub fn completed(&self) -> bool {
        self.completed
    }

    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }

    /// The assistant turn exactly as the API produced it, for replay.
    pub fn assistant_message(&self) -> Value {
        json!({"role": "assistant", "content": self.content.clone()})
    }

    pub fn accept(&mut self, event: &Value) -> Result<Vec<StreamSignal>, NativeRuntimeError> {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "error" => Err(classify_stream_error(event).error()),
            "content_block_start" => {
                let index = block_index(event)?;
                let block = event.get("content_block").ok_or_else(invalid_stream)?;
                let block_type = block.get("type").and_then(Value::as_str).unwrap_or_default();
                let state = match block_type {
                    "text" => BlockState::Text,
                    "tool_use" => BlockState::ToolUse {
                        id: bounded_id(block.get("id"))?,
                        name: bounded_id(block.get("name"))?,
                        input_json: String::new(),
                    },
                    "thinking" | "redacted_thinking" => BlockState::Thinking {
                        raw: block.as_object().cloned().ok_or_else(invalid_stream)?,
                    },
                    _ => BlockState::Ignored,
                };
                self.blocks.insert(index, state);
                Ok(Vec::new())
            }
            "content_block_delta" => {
                let index = block_index(event)?;
                let delta = event.get("delta").ok_or_else(invalid_stream)?;
                let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or_default();
                let Some(state) = self.blocks.get_mut(&index) else {
                    return Ok(Vec::new());
                };
                match (state, delta_type) {
                    (BlockState::Text, "text_delta") => {
                        let text = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or_else(invalid_stream)?;
                        self.text_bytes = self.text_bytes.saturating_add(text.len());
                        if self.text_bytes > self.max_text_bytes {
                            return Err(NativeRuntimeError::new(
                                NativeErrorCode::EventLimitExceeded,
                                "assistant output exceeded the native turn limit",
                                false,
                            ));
                        }
                        Ok(vec![StreamSignal::Text(text.to_string())])
                    }
                    (BlockState::ToolUse { input_json, .. }, "input_json_delta") => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .ok_or_else(invalid_stream)?;
                        input_json.push_str(partial);
                        if input_json.len() > MAX_TOOL_OUTPUT_BYTES {
                            return Err(NativeRuntimeError::new(
                                NativeErrorCode::EventLimitExceeded,
                                "tool input exceeded the Alfred limit",
                                false,
                            ));
                        }
                        Ok(Vec::new())
                    }
                    (BlockState::Thinking { raw }, "thinking_delta") => {
                        append_string(raw, "thinking", delta.get("thinking"));
                        Ok(Vec::new())
                    }
                    (BlockState::Thinking { raw }, "signature_delta") => {
                        append_string(raw, "signature", delta.get("signature"));
                        Ok(Vec::new())
                    }
                    _ => Ok(Vec::new()),
                }
            }
            "content_block_stop" => {
                let index = block_index(event)?;
                match self.blocks.remove(&index) {
                    Some(BlockState::Text) => {}
                    Some(BlockState::ToolUse {
                        id,
                        name,
                        input_json,
                    }) => {
                        let input_json = if input_json.trim().is_empty() {
                            "{}".to_string()
                        } else {
                            input_json
                        };
                        let input: Value = serde_json::from_str(&input_json).map_err(|_| {
                            NativeRuntimeError::new(
                                NativeErrorCode::InvalidRequest,
                                "the model produced an unparsable tool input object",
                                false,
                            )
                        })?;
                        self.content.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input,
                        }));
                        self.tool_calls.push(ToolCall {
                            id,
                            name,
                            input_json,
                        });
                    }
                    Some(BlockState::Thinking { raw }) => {
                        self.content.push(Value::Object(raw));
                    }
                    _ => {}
                }
                Ok(Vec::new())
            }
            "message_delta" => {
                if let Some(reason) = event
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.stop_reason = Some(reason.to_string());
                }
                Ok(Vec::new())
            }
            "message_stop" => {
                self.completed = true;
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Text blocks are rebuilt from the emitted deltas so the replayed
    /// assistant turn matches what the API produced.
    pub fn finish_text(&mut self, text: &str) {
        if !text.is_empty() {
            self.content
                .insert(0, json!({"type": "text", "text": text}));
        }
    }
}

fn append_string(raw: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    let Some(Value::String(fragment)) = value else {
        return;
    };
    match raw.get_mut(key) {
        Some(Value::String(existing)) => existing.push_str(fragment),
        _ => {
            raw.insert(key.to_string(), Value::String(fragment.clone()));
        }
    }
}

fn block_index(event: &Value) -> Result<u64, NativeRuntimeError> {
    event
        .get("index")
        .and_then(Value::as_u64)
        .filter(|index| *index < 1_024)
        .ok_or_else(invalid_stream)
}

fn bounded_id(value: Option<&Value>) -> Result<String, NativeRuntimeError> {
    let value = value.and_then(Value::as_str).unwrap_or_default();
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if valid {
        Ok(value.to_string())
    } else {
        Err(invalid_stream())
    }
}

fn invalid_stream() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidEvent,
        "provider stream event was malformed",
        false,
    )
}

/// Parses `GET /v1/models`. One page only: the harness caps a catalog at 512
/// entries and Alfred does not need the long tail of retired snapshots.
pub fn parse_model_catalog(body: &str) -> Result<Vec<crate::agents::native::NativeModel>, NativeRuntimeError> {
    let value: Value = serde_json::from_str(body).map_err(|_| {
        NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "Anthropic model catalog was not valid JSON",
            false,
        )
    })?;
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::ModelUnavailable,
                "Anthropic model catalog was missing its data array",
                false,
            )
        })?;
    if entries.len() > MAX_MODEL_CATALOG_ENTRIES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "Anthropic model catalog exceeded its entry limit",
            false,
        ));
    }
    let models = entries
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= MAX_MODEL_ID_BYTES)
                .ok_or_else(|| {
                    NativeRuntimeError::new(
                        NativeErrorCode::ModelUnavailable,
                        "Anthropic model catalog contained an invalid model id",
                        false,
                    )
                })?;
            let label = entry
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(id);
            if label.is_empty() || label.len() > MAX_MODEL_LABEL_BYTES {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::ModelUnavailable,
                    "Anthropic model catalog contained an invalid model label",
                    false,
                ));
            }
            Ok(crate::agents::native::NativeModel {
                id: id.to_string(),
                label: label.to_string(),
            })
        })
        .collect::<Result<Vec<_>, NativeRuntimeError>>()?;
    if models.is_empty() {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::ModelUnavailable,
            "Anthropic model catalog was empty for this API key",
            false,
        ));
    }
    Ok(models)
}

/// Screens a stored credential before it is used as an API key. Nothing here
/// echoes the key itself.
pub fn validate_api_key(key: &str) -> Result<(), NativeRuntimeError> {
    let valid = key.starts_with(API_KEY_PREFIX)
        && key.len() > API_KEY_PREFIX.len()
        && key.len() <= 512
        && key.trim() == key
        && key.bytes().all(|byte| byte.is_ascii_graphic());
    if valid {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "native Claude account does not hold an Anthropic API key",
            false,
        ))
    }
}

/// Belt-and-braces: any provider text Alfred keeps is redacted and bounded
/// before it can reach an event or an error.
pub fn safe_provider_text(value: &str, maximum: usize) -> String {
    let redacted = redact_text(value);
    if redacted.len() <= maximum {
        return redacted;
    }
    let mut end = maximum;
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    redacted[..end].to_string()
}

/// The Alfred tool contract version this provider encodes against.
pub const ENCODED_TOOL_CONTRACT_VERSION: u16 = TOOL_CONTRACT_VERSION;
