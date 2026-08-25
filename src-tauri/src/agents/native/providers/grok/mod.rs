//! Bounded xAI Responses API mapping for the native Grok harness.
//!
//! Live registration remains intentionally absent. xAI documents inference
//! API keys, but the frozen native-account contract has no approved way to
//! enter one without crossing the React/Tauri DTO boundary. The transport
//! seam and fixtures here keep the provider mapping reviewable without
//! scraping Grok Build or consumer Grok credentials.

use crate::agent_accounts::resolver::NativeAgentCredential;
use crate::agents::native::{
    redact_text, AlfredToolKind, AlfredToolRequest, NativeAgentRuntime,
    NativeCapabilities, NativeContentClass, NativeErrorCode, NativeEvent,
    NativeEventKind, NativeModel, NativeRuntimeDescriptor, NativeRuntimeError,
    NativeSessionMode, NativeTurnHost, NativeTurnOutcome, NativeTurnRequest,
    NativeUsageSnapshot, ResolvedNativeAccount, NATIVE_CAPABILITY_CONTRACT_VERSION,
    NATIVE_EVENT_CONTRACT_VERSION, NATIVE_REQUEST_CONTRACT_VERSION,
};
use crate::agents::AgentProvider;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const GROK_RUNTIME_ID: &str = "xai-responses";
const GROK_RUNTIME_VERSION: &str = "0.1.0-blocked-account-setup";
const MAX_TOOL_ROUNDS: usize = 8;
const MAX_PROVIDER_FRAMES: usize = 2_048;
const MAX_PROVIDER_FRAME_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokTransportErrorKind {
    Unauthorized,
    Revoked,
    RateLimited,
    Timeout,
    Cancelled,
    Safety,
    Malformed,
    Oversized,
    Provider,
}

#[derive(Clone, PartialEq, Eq)]
pub struct GrokTransportError {
    pub kind: GrokTransportErrorKind,
}

impl std::fmt::Debug for GrokTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GrokTransportError")
            .field("kind", &self.kind)
            .field("detail", &"[REDACTED]")
            .finish()
    }
}

impl GrokTransportError {
    pub fn new(kind: GrokTransportErrorKind, _detail: impl AsRef<str>) -> Self {
        Self { kind }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokModelRecord {
    pub id: String,
    pub owned_by: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GrokStreamEvent {
    ResponseCreated { response_id: String },
    TextDelta(String),
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum GrokInputItem {
    Message {
        role: GrokMessageRole,
        content: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrokMessageRole {
    Developer,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GrokResponsesRequest {
    pub model: String,
    pub input: Vec<GrokInputItem>,
    pub tools: Vec<Value>,
    pub stream: bool,
    pub store: bool,
    pub parallel_tool_calls: bool,
    pub max_output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
}

pub trait GrokResponsesTransport: Send + Sync {
    fn validate_api_key(&self, api_key: &str) -> Result<(), GrokTransportError>;
    fn list_language_models(
        &self,
        api_key: &str,
    ) -> Result<Vec<GrokModelRecord>, GrokTransportError>;
    fn stream_response(
        &self,
        api_key: &str,
        request: &GrokResponsesRequest,
        cancellation: &crate::agents::native::NativeCancellation,
    ) -> Result<Vec<Result<GrokStreamEvent, GrokTransportError>>, GrokTransportError>;
}

pub struct GrokNativeRuntime<T> {
    transport: T,
}

impl<T> GrokNativeRuntime<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T: GrokResponsesTransport> NativeAgentRuntime for GrokNativeRuntime<T> {
    fn descriptor(&self) -> NativeRuntimeDescriptor {
        NativeRuntimeDescriptor {
            runtime_id: GROK_RUNTIME_ID.into(),
            runtime_version: GROK_RUNTIME_VERSION.into(),
            request_contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            event_contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            provider: AgentProvider::Grok,
            capabilities: NativeCapabilities {
                contract_version: NATIVE_CAPABILITY_CONTRACT_VERSION,
                supports_api_key: true,
                supports_model_list: true,
                supports_tool_calls: true,
                supports_approval_events: true,
                supports_native_filesystem: true,
                supports_native_shell: true,
                supports_patch: true,
                ..NativeCapabilities::default()
            },
        }
    }

    fn validate_account(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<(), NativeRuntimeError> {
        let api_key = api_key(account)?;
        self.transport
            .validate_api_key(api_key)
            .map_err(map_transport_error)
    }

    fn discover_models(
        &self,
        account: &ResolvedNativeAccount,
    ) -> Result<Vec<NativeModel>, NativeRuntimeError> {
        let api_key = api_key(account)?;
        let models = self
            .transport
            .list_language_models(api_key)
            .map_err(map_transport_error)?;
        let models = models
            .into_iter()
            .filter(|model| model.owned_by == "xai")
            .map(|model| NativeModel {
                label: model.id.clone(),
                id: model.id,
            })
            .collect::<Vec<_>>();
        if models.is_empty() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::ModelUnavailable,
                "xAI returned no language models for this API key",
                false,
            ));
        }
        Ok(models)
    }

    fn run_turn(
        &self,
        account: &ResolvedNativeAccount,
        request: &NativeTurnRequest,
        host: &mut dyn NativeTurnHost,
    ) -> Result<NativeTurnOutcome, NativeRuntimeError> {
        if request.session_mode != NativeSessionMode::Ephemeral || request.session_id.is_some() {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::SessionUnavailable,
                "the xAI native runtime currently supports ephemeral turns only",
                false,
            ));
        }
        let api_key = api_key(account)?;
        host.cancellation().checkpoint()?;
        host.emit(NativeEvent::new(0, NativeEventKind::TurnStarted))?;

        let mut provider_request = initial_request(request)?;
        for round in 0..=MAX_TOOL_ROUNDS {
            host.cancellation().checkpoint()?;
            let frames = self
                .transport
                .stream_response(api_key, &provider_request, host.cancellation())
                .map_err(map_transport_error)?;
            if frames.len() > MAX_PROVIDER_FRAMES {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "xAI stream exceeded the provider frame limit",
                    false,
                ));
            }

            let mut response_id = None;
            let mut function_call = None;
            let mut completed = false;
            for frame in frames {
                host.cancellation().checkpoint()?;
                match frame.map_err(map_transport_error)? {
                    GrokStreamEvent::ResponseCreated {
                        response_id: candidate,
                    } => {
                        validate_provider_id(&candidate, "xAI response id")?;
                        response_id = Some(candidate);
                    }
                    GrokStreamEvent::TextDelta(text) => {
                        let mut event = NativeEvent::new(0, NativeEventKind::AssistantDelta);
                        event.content_class = Some(NativeContentClass::Assistant);
                        event.text = Some(redact_xai_text(&text));
                        host.emit(event)?;
                    }
                    GrokStreamEvent::FunctionCall {
                        call_id,
                        name,
                        arguments,
                    } => {
                        if function_call.is_some() {
                            return Err(invalid_provider_event(
                                "xAI returned parallel function calls after they were disabled",
                            ));
                        }
                        function_call = Some((call_id, name, arguments));
                    }
                    GrokStreamEvent::Completed => completed = true,
                }
            }
            if !completed {
                return Err(invalid_provider_event(
                    "xAI stream ended before response completion",
                ));
            }

            let Some((call_id, name, arguments)) = function_call else {
                host.emit(NativeEvent::new(0, NativeEventKind::TurnCompleted))?;
                return Ok(NativeTurnOutcome { session_id: None });
            };
            if round == MAX_TOOL_ROUNDS {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "xAI exceeded the Alfred tool-round limit",
                    false,
                ));
            }
            let response_id = response_id.ok_or_else(|| {
                invalid_provider_event("xAI function call omitted its response id")
            })?;
            let tool_request = tool_request(&call_id, &name, &arguments)?;
            let result = host.invoke_tool(tool_request)?;
            let output = serde_json::to_string(&result).map_err(|_| {
                invalid_provider_event("Alfred tool result could not be encoded for xAI")
            })?;
            let output = redact_xai_text(&output);
            provider_request = GrokResponsesRequest {
                model: request.model.clone(),
                input: vec![GrokInputItem::FunctionCallOutput { call_id, output }],
                tools: tool_definitions(request),
                stream: true,
                store: false,
                parallel_tool_calls: false,
                max_output_tokens: MAX_OUTPUT_TOKENS,
                previous_response_id: Some(response_id),
            };
        }
        unreachable!("bounded tool loop returns on every terminal branch")
    }

    fn usage_snapshot(
        &self,
        _account: &ResolvedNativeAccount,
    ) -> Result<NativeUsageSnapshot, NativeRuntimeError> {
        // The inference key exposes per-response usage and cost, not the
        // account-wide window represented by NativeUsageSnapshot. xAI's
        // historical usage endpoint requires a separate Management API key.
        Ok(NativeUsageSnapshot::unavailable())
    }
}

fn api_key(account: &ResolvedNativeAccount) -> Result<&str, NativeRuntimeError> {
    if account.provider != AgentProvider::Grok {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::AccountMismatch,
            "native account belongs to a different provider",
            false,
        ));
    }
    let key = account
        .credential
        .downcast_ref::<NativeAgentCredential>()
        .and_then(NativeAgentCredential::access_token)
        .or_else(|| test_api_key(account))
        .ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "xAI API key is unavailable; reconnect the native account",
                false,
            )
        })?;
    if key.len() < 20
        || key.len() > 512
        || !key.starts_with("xai-")
        || key.chars().any(char::is_whitespace)
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "xAI API key is malformed; rotate it in the xAI Console and reconnect",
            false,
        ));
    }
    Ok(key)
}

#[cfg(not(test))]
fn test_api_key(_account: &ResolvedNativeAccount) -> Option<&str> {
    None
}

#[cfg(test)]
fn test_api_key(account: &ResolvedNativeAccount) -> Option<&str> {
    account
        .credential
        .downcast_ref::<TestOnlyGrokCredential>()
        .map(|credential| credential.api_key.as_str())
}

fn initial_request(request: &NativeTurnRequest) -> Result<GrokResponsesRequest, NativeRuntimeError> {
    let mut input = Vec::with_capacity(request.context.len());
    for block in &request.context {
        if redact_xai_text(&block.content) != block.content {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "xAI API keys are prohibited in native prompt context",
                false,
            ));
        }
        let role = match block.role {
            crate::agents::native::NativeContextRole::System
            | crate::agents::native::NativeContextRole::Skill => GrokMessageRole::Developer,
            crate::agents::native::NativeContextRole::User => GrokMessageRole::User,
            crate::agents::native::NativeContextRole::Assistant => GrokMessageRole::Assistant,
            crate::agents::native::NativeContextRole::Tool => {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::InvalidRequest,
                    "tool context must use a documented xAI function-call continuation",
                    false,
                ))
            }
        };
        input.push(GrokInputItem::Message {
            role,
            content: block.content.clone(),
        });
    }
    Ok(GrokResponsesRequest {
        model: request.model.clone(),
        input,
        tools: tool_definitions(request),
        stream: true,
        store: false,
        parallel_tool_calls: false,
        max_output_tokens: MAX_OUTPUT_TOKENS,
        previous_response_id: None,
    })
}

fn tool_definitions(request: &NativeTurnRequest) -> Vec<Value> {
    let mut tools = Vec::new();
    if request.tool_capabilities.filesystem {
        tools.extend([
            function_tool("alfred_file_read", "Read a workspace file", path_schema()),
            function_tool(
                "alfred_file_write",
                "Write a workspace file",
                path_and_text_schema("content"),
            ),
            function_tool(
                "alfred_file_edit",
                "Edit a workspace file",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "old_text": {"type": "string"},
                        "new_text": {"type": "string"}
                    },
                    "required": ["path", "old_text", "new_text"],
                    "additionalProperties": false
                }),
            ),
            function_tool(
                "alfred_directory_list",
                "List a workspace directory",
                path_schema(),
            ),
        ]);
    }
    if request.tool_capabilities.shell {
        tools.push(function_tool(
            "alfred_shell",
            "Run an argv command in a workspace directory",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "command": {"type": "array", "items": {"type": "string"}, "minItems": 1}
                },
                "required": ["path", "command"],
                "additionalProperties": false
            }),
        ));
    }
    if request.tool_capabilities.patch {
        tools.push(function_tool(
            "alfred_apply_patch",
            "Apply a bounded patch inside the workspace",
            path_and_text_schema("patch"),
        ));
    }
    tools
}

fn function_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": parameters,
        "strict": true
    })
}

fn path_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"path": {"type": "string"}},
        "required": ["path"],
        "additionalProperties": false
    })
}

fn path_and_text_schema(field: &str) -> Value {
    let mut properties = Map::new();
    properties.insert("path".into(), json!({"type": "string"}));
    properties.insert(field.into(), json!({"type": "string"}));
    json!({
        "type": "object",
        "properties": properties,
        "required": ["path", field],
        "additionalProperties": false
    })
}

fn tool_request(
    call_id: &str,
    name: &str,
    arguments: &str,
) -> Result<AlfredToolRequest, NativeRuntimeError> {
    validate_provider_id(call_id, "xAI function call id")?;
    if redact_xai_text(arguments) != arguments {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::PermissionDenied,
            "xAI credential-shaped material is prohibited in tool arguments",
            false,
        ));
    }
    let mut input = serde_json::from_str::<Map<String, Value>>(arguments).map_err(|_| {
        invalid_provider_event("xAI function call arguments were malformed")
    })?;
    let path = take_string(&mut input, "path")?.map(PathBuf::from);
    let (kind, arguments) = match name {
        "alfred_file_read" => {
            require_only_string_fields(&input, &[])?;
            (AlfredToolKind::FileRead, Vec::new())
        }
        "alfred_file_write" => {
            require_only_string_fields(&input, &["content"])?;
            (AlfredToolKind::FileWrite, Vec::new())
        }
        "alfred_file_edit" => {
            require_only_string_fields(&input, &["old_text", "new_text"])?;
            (AlfredToolKind::FileEdit, Vec::new())
        }
        "alfred_directory_list" => {
            require_only_string_fields(&input, &[])?;
            (AlfredToolKind::DirectoryList, Vec::new())
        }
        "alfred_shell" => {
            let command = take_string_array(&mut input, "command")?;
            require_only_string_fields(&input, &[])?;
            (AlfredToolKind::Shell, command)
        }
        "alfred_apply_patch" => {
            require_only_string_fields(&input, &["patch"])?;
            (AlfredToolKind::ApplyPatch, Vec::new())
        }
        _ => return Err(invalid_provider_event("xAI requested an unknown Alfred function")),
    };
    let mut request = AlfredToolRequest::new(call_id, kind, name);
    request.path = path;
    request.arguments = arguments;
    request.input = input;
    Ok(request)
}

fn require_only_string_fields(
    input: &Map<String, Value>,
    fields: &[&str],
) -> Result<(), NativeRuntimeError> {
    if input.len() != fields.len()
        || fields
            .iter()
            .any(|field| !matches!(input.get(*field), Some(Value::String(_))))
    {
        Err(invalid_provider_event(
            "xAI function call arguments did not match the Alfred tool schema",
        ))
    } else {
        Ok(())
    }
}

fn take_string(
    input: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<String>, NativeRuntimeError> {
    match input.remove(key) {
        None => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value)),
        Some(_) => Err(invalid_provider_event(
            "xAI function call contained an invalid string field",
        )),
    }
}

fn take_string_array(
    input: &mut Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, NativeRuntimeError> {
    match input.remove(key) {
        Some(Value::Array(values)) if !values.is_empty() => values
            .into_iter()
            .map(|value| match value {
                Value::String(value) if !value.is_empty() => Ok(value),
                _ => Err(invalid_provider_event(
                    "xAI function call contained invalid command arguments",
                )),
            })
            .collect(),
        _ => Err(invalid_provider_event(
            "xAI function call omitted command arguments",
        )),
    }
}

fn validate_provider_id(value: &str, label: &str) -> Result<(), NativeRuntimeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        Err(invalid_provider_event(format!("{label} is invalid")))
    } else {
        Ok(())
    }
}

fn map_transport_error(error: GrokTransportError) -> NativeRuntimeError {
    match error.kind {
        GrokTransportErrorKind::Unauthorized => NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "xAI rejected the API key; reconnect with a current key",
            false,
        ),
        GrokTransportErrorKind::Revoked => NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "xAI API key was disabled or revoked; rotate it in the xAI Console",
            false,
        ),
        GrokTransportErrorKind::RateLimited => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "xAI rate limit reached; retry after the provider backoff window",
            true,
        ),
        GrokTransportErrorKind::Timeout => NativeRuntimeError::timed_out(),
        GrokTransportErrorKind::Cancelled => NativeRuntimeError::cancelled(),
        GrokTransportErrorKind::Safety => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "xAI rejected the request under its safety policy",
            false,
        ),
        GrokTransportErrorKind::Malformed => {
            invalid_provider_event("xAI returned a malformed streaming event")
        }
        GrokTransportErrorKind::Oversized => NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "xAI streaming response exceeded the provider byte limit",
            false,
        ),
        GrokTransportErrorKind::Provider => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "xAI provider request failed",
            true,
        ),
    }
}

fn invalid_provider_event(message: impl Into<String>) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidEvent, message, false)
}

fn redact_xai_text(value: &str) -> String {
    let value = redact_text(value);
    let lower = value.to_ascii_lowercase();
    let mut spans = Vec::new();
    let mut search = 0usize;
    while let Some(offset) = lower[search..].find("xai-") {
        let start = search + offset;
        let at_boundary = start == 0
            || value[..start]
                .chars()
                .next_back()
                .is_some_and(|character| {
                    character.is_whitespace()
                        || matches!(
                            character,
                            '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '{' | '}' | '[' | ']'
                        )
                });
        let end = value[start..]
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, ',' | ';' | '"' | '\'' | ')' | '}')
            })
            .map(|offset| start + offset)
            .unwrap_or(value.len());
        if at_boundary {
            spans.push((start, end));
        }
        search = (start + 4).min(value.len());
        if search >= value.len() {
            break;
        }
    }
    if spans.is_empty() {
        return value;
    }
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

/// Parses one `data:` payload from the documented Responses API SSE stream.
/// Raw provider JSON never reaches a normalized Alfred event.
pub fn parse_responses_sse_data(data: &str) -> Result<Option<GrokStreamEvent>, GrokTransportError> {
    let data = data.trim();
    if data.len() > MAX_PROVIDER_FRAME_BYTES {
        return Err(GrokTransportError::new(
            GrokTransportErrorKind::Oversized,
            "xAI SSE frame exceeded its byte limit",
        ));
    }
    if data == "[DONE]" {
        return Ok(None);
    }
    let value = serde_json::from_str::<Value>(data).map_err(|_| {
        GrokTransportError::new(GrokTransportErrorKind::Malformed, "invalid xAI SSE JSON")
    })?;
    let event_type = value.get("type").and_then(Value::as_str).ok_or_else(|| {
        GrokTransportError::new(GrokTransportErrorKind::Malformed, "missing xAI event type")
    })?;
    match event_type {
        "response.created" => {
            let response_id = value
                .pointer("/response/id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    GrokTransportError::new(
                        GrokTransportErrorKind::Malformed,
                        "missing xAI response id",
                    )
                })?;
            Ok(Some(GrokStreamEvent::ResponseCreated {
                response_id: response_id.into(),
            }))
        }
        "response.output_text.delta" | "response.text.delta" => {
            let delta = value.get("delta").and_then(Value::as_str).ok_or_else(|| {
                GrokTransportError::new(
                    GrokTransportErrorKind::Malformed,
                    "missing xAI output delta",
                )
            })?;
            Ok(Some(GrokStreamEvent::TextDelta(delta.into())))
        }
        "response.output_item.done"
            if value.pointer("/item/type").and_then(Value::as_str) == Some("function_call") =>
        {
            let field = |name: &str| {
                value
                    .pointer(&format!("/item/{name}"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| {
                        GrokTransportError::new(
                            GrokTransportErrorKind::Malformed,
                            "incomplete xAI function call",
                        )
                    })
            };
            Ok(Some(GrokStreamEvent::FunctionCall {
                call_id: field("call_id")?,
                name: field("name")?,
                arguments: field("arguments")?,
            }))
        }
        "response.completed" => Ok(Some(GrokStreamEvent::Completed)),
        "error" | "response.failed" => {
            let code = value
                .pointer("/error/code")
                .or_else(|| value.pointer("/response/error/code"))
                .and_then(Value::as_str)
                .unwrap_or("provider_error");
            let message = value
                .pointer("/error/message")
                .or_else(|| value.pointer("/response/error/message"))
                .and_then(Value::as_str)
                .unwrap_or("xAI provider error");
            let kind = match code {
                "invalid_api_key" | "unauthorized" => GrokTransportErrorKind::Unauthorized,
                "api_key_disabled" | "api_key_revoked" => GrokTransportErrorKind::Revoked,
                "rate_limit_exceeded" => GrokTransportErrorKind::RateLimited,
                "content_policy_violation"
                | "safety_violation"
                | "usage_guidelines_violation" => {
                    GrokTransportErrorKind::Safety
                }
                _ => GrokTransportErrorKind::Provider,
            };
            Err(GrokTransportError::new(kind, message))
        }
        // Metadata/progress events are intentionally ignored. Reasoning events
        // never become assistant deltas or persisted native metadata.
        "response.in_progress"
        | "response.output_item.added"
        | "response.content_part.added"
        | "response.output_text.done"
        | "response.content_part.done"
        | "response.output_item.done" => Ok(None),
        _ if event_type.contains("reasoning") => Ok(None),
        _ => Err(GrokTransportError::new(
            GrokTransportErrorKind::Malformed,
            "unknown xAI streaming event",
        )),
    }
}

#[cfg(test)]
#[derive(Debug)]
struct TestOnlyGrokCredential {
    api_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::native::{
        AlfredApprovalDecision, AlfredApprovalHandler, AlfredApprovalRequest,
        AlfredToolExecutor, AlfredToolResult, AlfredToolStatus, NativeAccountResolver,
        NativeApprovalPolicy, NativeCancellation, NativeContextBlock, NativeContextRole,
        NativeCredential, NativeEventLimits, NativePermissionProfile, NativeRuntimeRegistry,
        NativeToolCapabilitySet, NativeUsageState, TOOL_CONTRACT_VERSION,
    };
    use crate::agents::{AgentHarness, OpaqueAgentAccountRef};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const TEST_KEY: &str = "xai-test-only-secret-credential";
    const TEST_MODEL: &str = "grok-test-model";

    struct FixtureTransport {
        validation: Mutex<Result<(), GrokTransportError>>,
        models: Mutex<Result<Vec<GrokModelRecord>, GrokTransportError>>,
        streams: Mutex<VecDeque<Result<Vec<Result<GrokStreamEvent, GrokTransportError>>, GrokTransportError>>>,
        requests: Mutex<Vec<GrokResponsesRequest>>,
    }

    impl Default for FixtureTransport {
        fn default() -> Self {
            Self {
                validation: Mutex::new(Ok(())),
                models: Mutex::new(Ok(vec![GrokModelRecord {
                    id: TEST_MODEL.into(),
                    owned_by: "xai".into(),
                }])),
                streams: Mutex::new(VecDeque::new()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl GrokResponsesTransport for Arc<FixtureTransport> {
        fn validate_api_key(&self, api_key: &str) -> Result<(), GrokTransportError> {
            assert_eq!(api_key, TEST_KEY);
            self.validation.lock().unwrap().clone()
        }

        fn list_language_models(
            &self,
            api_key: &str,
        ) -> Result<Vec<GrokModelRecord>, GrokTransportError> {
            assert_eq!(api_key, TEST_KEY);
            self.models.lock().unwrap().clone()
        }

        fn stream_response(
            &self,
            api_key: &str,
            request: &GrokResponsesRequest,
            cancellation: &NativeCancellation,
        ) -> Result<Vec<Result<GrokStreamEvent, GrokTransportError>>, GrokTransportError> {
            assert_eq!(api_key, TEST_KEY);
            if cancellation.is_cancelled() {
                return Err(GrokTransportError::new(
                    GrokTransportErrorKind::Cancelled,
                    "cancelled",
                ));
            }
            self.requests.lock().unwrap().push(request.clone());
            self.streams
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(GrokTransportError::new(
                        GrokTransportErrorKind::Provider,
                        "missing fixture stream",
                    ))
                })
        }
    }

    struct FixtureResolver {
        account_ref: OpaqueAgentAccountRef,
    }

    impl NativeAccountResolver for FixtureResolver {
        fn resolve(
            &self,
            account_ref: &OpaqueAgentAccountRef,
            provider: AgentProvider,
        ) -> Result<ResolvedNativeAccount, NativeRuntimeError> {
            if provider != AgentProvider::Grok || account_ref != &self.account_ref {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::AccountMismatch,
                    "fixture account mismatch",
                    false,
                ));
            }
            Ok(test_account(self.account_ref.clone()))
        }
    }

    struct FixtureApproval(AlfredApprovalDecision);

    impl AlfredApprovalHandler for FixtureApproval {
        fn decide(
            &self,
            _request: &AlfredApprovalRequest,
            cancellation: &NativeCancellation,
        ) -> Result<AlfredApprovalDecision, NativeRuntimeError> {
            cancellation.checkpoint()?;
            Ok(self.0)
        }
    }

    #[derive(Default)]
    struct FixtureToolExecutor {
        calls: AtomicUsize,
    }

    impl AlfredToolExecutor for FixtureToolExecutor {
        fn execute(
            &self,
            request: &AlfredToolRequest,
            cancellation: &NativeCancellation,
        ) -> Result<AlfredToolResult, NativeRuntimeError> {
            cancellation.checkpoint()?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(AlfredToolResult {
                contract_version: TOOL_CONTRACT_VERSION,
                request_id: request.request_id.clone(),
                status: AlfredToolStatus::Completed,
                output: "workspace".into(),
                exit_code: Some(0),
                truncated: false,
                metadata: Map::new(),
            })
        }
    }

    fn account_ref() -> OpaqueAgentAccountRef {
        OpaqueAgentAccountRef::parse("account_grok_fixture").unwrap()
    }

    fn test_account(account_ref: OpaqueAgentAccountRef) -> ResolvedNativeAccount {
        ResolvedNativeAccount {
            account_ref,
            provider: AgentProvider::Grok,
            credential: NativeCredential::new(TestOnlyGrokCredential {
                api_key: TEST_KEY.into(),
            }),
        }
    }

    fn request() -> NativeTurnRequest {
        let working_directory = std::env::current_dir().unwrap();
        NativeTurnRequest {
            contract_version: NATIVE_REQUEST_CONTRACT_VERSION,
            harness: AgentHarness::Alfred,
            harness_version: "test".into(),
            runtime_version: GROK_RUNTIME_VERSION.into(),
            provider: AgentProvider::Grok,
            account_ref: account_ref(),
            run_id: "run_grok_fixture".into(),
            node_id: "node_grok_fixture".into(),
            model: TEST_MODEL.into(),
            prompt: "Inspect the workspace".into(),
            context: vec![NativeContextBlock {
                role: NativeContextRole::User,
                content: "Inspect the workspace".into(),
                name: None,
            }],
            working_directory: working_directory.clone(),
            allowed_workspace_roots: vec![working_directory],
            permission_profile: NativePermissionProfile::default(),
            tool_capabilities: NativeToolCapabilitySet::default(),
            session_mode: NativeSessionMode::Ephemeral,
            session_id: None,
            event_limits: NativeEventLimits::default(),
            timeout_ms: 30_000,
            cancellation: Some(NativeCancellation::new("grok_fixture", Duration::from_secs(30)).unwrap()),
        }
    }

    fn execute(
        transport: Arc<FixtureTransport>,
        request: &NativeTurnRequest,
        approval: AlfredApprovalDecision,
        executor: &FixtureToolExecutor,
    ) -> Result<crate::agents::native::NativeExecutionResult, NativeRuntimeError> {
        let registry = NativeRuntimeRegistry::default();
        registry
            .register(Arc::new(GrokNativeRuntime::new(transport)))
            .unwrap();
        registry.execute_turn(
            request,
            &FixtureResolver {
                account_ref: account_ref(),
            },
            executor,
            &FixtureApproval(approval),
            &mut |_| {},
        )
    }

    fn final_stream(text: &str) -> Vec<Result<GrokStreamEvent, GrokTransportError>> {
        vec![
            Ok(GrokStreamEvent::ResponseCreated {
                response_id: "resp_final".into(),
            }),
            Ok(GrokStreamEvent::TextDelta(text.into())),
            Ok(GrokStreamEvent::Completed),
        ]
    }

    #[test]
    fn descriptor_models_and_usage_state_are_honest() {
        let transport = Arc::new(FixtureTransport::default());
        let runtime = GrokNativeRuntime::new(transport.clone());
        let descriptor = runtime.descriptor();
        assert_eq!(descriptor.provider, AgentProvider::Grok);
        assert!(descriptor.capabilities.supports_api_key);
        assert!(!descriptor.capabilities.supports_oauth);
        assert!(descriptor.capabilities.supports_model_list);
        assert!(descriptor.capabilities.supports_tool_calls);
        assert!(!descriptor.capabilities.supports_sessions);
        assert!(!descriptor.capabilities.supports_usage);

        let account = test_account(account_ref());
        assert_eq!(runtime.discover_models(&account).unwrap()[0].id, TEST_MODEL);
        assert_eq!(
            runtime.usage_snapshot(&account).unwrap().state,
            NativeUsageState::Unavailable
        );

        *transport.models.lock().unwrap() = Ok(Vec::new());
        assert_eq!(
            runtime.discover_models(&account).unwrap_err().code,
            NativeErrorCode::ModelUnavailable
        );
    }

    #[test]
    fn auth_failure_and_revoked_key_map_to_account_recovery() {
        for (kind, expected) in [
            (GrokTransportErrorKind::Unauthorized, "reconnect"),
            (GrokTransportErrorKind::Revoked, "rotate"),
        ] {
            let transport = Arc::new(FixtureTransport::default());
            *transport.validation.lock().unwrap() = Err(GrokTransportError::new(
                kind,
                format!("rejected {TEST_KEY}"),
            ));
            let error = GrokNativeRuntime::new(transport)
                .validate_account(&test_account(account_ref()))
                .unwrap_err();
            assert_eq!(error.code, NativeErrorCode::AccountUnavailable);
            assert!(!error.retryable);
            assert!(error.message.contains(expected));
            assert!(!error.message.contains(TEST_KEY));
        }
    }

    #[test]
    fn rate_limit_provider_and_safety_errors_are_classified_and_redacted() {
        for (kind, code, retryable) in [
            (GrokTransportErrorKind::RateLimited, NativeErrorCode::ProviderUnavailable, true),
            (GrokTransportErrorKind::Safety, NativeErrorCode::ProviderUnavailable, false),
            (GrokTransportErrorKind::Provider, NativeErrorCode::ProviderUnavailable, true),
        ] {
            let transport = Arc::new(FixtureTransport::default());
            transport.streams.lock().unwrap().push_back(Err(GrokTransportError::new(
                kind,
                format!("provider echoed {TEST_KEY} Authorization: Bearer bearer-secret"),
            )));
            let error = execute(
                transport,
                &request(),
                AlfredApprovalDecision::Deny,
                &FixtureToolExecutor::default(),
            )
            .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
            assert!(!error.message.contains(TEST_KEY));
            assert!(!error.message.contains("bearer-secret"));
        }
    }

    #[test]
    fn timeout_and_cancellation_have_stable_codes() {
        let transport = Arc::new(FixtureTransport::default());
        transport.streams.lock().unwrap().push_back(Err(GrokTransportError::new(
            GrokTransportErrorKind::Timeout,
            "deadline",
        )));
        assert_eq!(
            execute(
                transport,
                &request(),
                AlfredApprovalDecision::Deny,
                &FixtureToolExecutor::default(),
            )
            .unwrap_err()
            .code,
            NativeErrorCode::TimedOut
        );

        let transport = Arc::new(FixtureTransport::default());
        let cancelled = request();
        cancelled.cancellation.as_ref().unwrap().cancel();
        assert_eq!(
            execute(
                transport,
                &cancelled,
                AlfredApprovalDecision::Deny,
                &FixtureToolExecutor::default(),
            )
            .unwrap_err()
            .code,
            NativeErrorCode::Cancelled
        );

        let transport = Arc::new(FixtureTransport::default());
        transport.streams.lock().unwrap().push_back(Err(GrokTransportError::new(
            GrokTransportErrorKind::Cancelled,
            "cancelled by transport",
        )));
        assert_eq!(
            execute(
                transport,
                &request(),
                AlfredApprovalDecision::Deny,
                &FixtureToolExecutor::default(),
            )
            .unwrap_err()
            .code,
            NativeErrorCode::Cancelled
        );
    }

    #[test]
    fn malformed_and_oversized_streams_are_refused() {
        assert_eq!(
            parse_responses_sse_data("not json").unwrap_err().kind,
            GrokTransportErrorKind::Malformed
        );
        assert_eq!(
            parse_responses_sse_data(r#"{"type":"undocumented.private_event"}"#)
                .unwrap_err()
                .kind,
            GrokTransportErrorKind::Malformed
        );
        assert_eq!(
            parse_responses_sse_data(&"x".repeat(MAX_PROVIDER_FRAME_BYTES + 1))
                .unwrap_err()
                .kind,
            GrokTransportErrorKind::Oversized
        );

        let transport = Arc::new(FixtureTransport::default());
        transport.streams.lock().unwrap().push_back(Ok(vec![Ok(
            GrokStreamEvent::TextDelta("stream without completion".into()),
        )]));
        assert_eq!(
            execute(
                transport,
                &request(),
                AlfredApprovalDecision::Deny,
                &FixtureToolExecutor::default(),
            )
            .unwrap_err()
            .code,
            NativeErrorCode::InvalidEvent
        );

        let transport = Arc::new(FixtureTransport::default());
        transport
            .streams
            .lock()
            .unwrap()
            .push_back(Ok(final_stream("0123456789")));
        let mut bounded = request();
        bounded.event_limits.max_text_bytes = 8;
        assert_eq!(
            execute(
                transport,
                &bounded,
                AlfredApprovalDecision::Deny,
                &FixtureToolExecutor::default(),
            )
            .unwrap_err()
            .code,
            NativeErrorCode::EventLimitExceeded
        );
    }

    #[test]
    fn documented_function_call_uses_alfred_approval_and_denial() {
        let transport = Arc::new(FixtureTransport::default());
        transport.streams.lock().unwrap().extend([
            Ok(vec![
                Ok(GrokStreamEvent::ResponseCreated {
                    response_id: "resp_tool".into(),
                }),
                Ok(GrokStreamEvent::FunctionCall {
                    call_id: "call_shell".into(),
                    name: "alfred_shell".into(),
                    arguments: r#"{"path":".","command":["pwd"]}"#.into(),
                }),
                Ok(GrokStreamEvent::Completed),
            ]),
            Ok(final_stream("The shell request was denied safely.")),
        ]);
        let mut tool_request = request();
        tool_request.permission_profile.shell = NativeApprovalPolicy::Ask;
        tool_request.tool_capabilities.shell = true;
        let executor = FixtureToolExecutor::default();
        let result = execute(
            transport.clone(),
            &tool_request,
            AlfredApprovalDecision::Deny,
            &executor,
        )
        .unwrap();
        assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
        assert_eq!(result.output, "The shell request was denied safely.");
        assert!(result.events.iter().any(|event| {
            event.kind == NativeEventKind::ApprovalResolved && event.approved == Some(false)
        }));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0].store);
        assert!(!requests[0].parallel_tool_calls);
        assert_eq!(requests[0].max_output_tokens, MAX_OUTPUT_TOKENS);
        assert_eq!(requests[1].previous_response_id.as_deref(), Some("resp_tool"));
        let serialized = serde_json::to_string(&requests[1]).unwrap();
        assert!(serialized.contains("denied"));
        assert!(!serialized.contains(TEST_KEY));
    }

    #[test]
    fn sse_parser_accepts_documented_text_tool_completion_and_filters_reasoning() {
        assert_eq!(
            parse_responses_sse_data(
                r#"{"type":"response.output_text.delta","delta":"hello"}"#
            )
            .unwrap(),
            Some(GrokStreamEvent::TextDelta("hello".into()))
        );
        assert!(matches!(
            parse_responses_sse_data(
                r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_1","name":"alfred_file_read","arguments":"{\"path\":\"README.md\"}"}}"#
            )
            .unwrap(),
            Some(GrokStreamEvent::FunctionCall { .. })
        ));
        assert_eq!(
            parse_responses_sse_data(r#"{"type":"response.completed","response":{}}"#)
                .unwrap(),
            Some(GrokStreamEvent::Completed)
        );
        assert_eq!(
            parse_responses_sse_data(r#"{"type":"response.reasoning.delta","delta":"private"}"#)
                .unwrap(),
            None
        );
    }

    #[test]
    fn provider_error_objects_never_render_xai_keys() {
        let error = GrokTransportError::new(
            GrokTransportErrorKind::Provider,
            format!("request failed for {TEST_KEY}"),
        );
        let debug = format!("{error:?}");
        let mapped = map_transport_error(error);
        assert!(!debug.contains(TEST_KEY));
        assert!(!mapped.message.contains(TEST_KEY));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn assistant_output_redacts_xai_keys_and_prompt_context_refuses_them() {
        let transport = Arc::new(FixtureTransport::default());
        transport
            .streams
            .lock()
            .unwrap()
            .push_back(Ok(final_stream(&format!("echoed {TEST_KEY}"))));
        let result = execute(
            transport,
            &request(),
            AlfredApprovalDecision::Deny,
            &FixtureToolExecutor::default(),
        )
        .unwrap();
        assert_eq!(result.output, "echoed [REDACTED]");
        assert!(!serde_json::to_string(&result.events).unwrap().contains(TEST_KEY));

        let transport = Arc::new(FixtureTransport::default());
        let mut secret_context = request();
        secret_context.prompt = format!("use {TEST_KEY}");
        secret_context.context[0].content = secret_context.prompt.clone();
        let error = execute(
            transport,
            &secret_context,
            AlfredApprovalDecision::Deny,
            &FixtureToolExecutor::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, NativeErrorCode::InvalidRequest);
        assert!(!error.message.contains(TEST_KEY));
    }
}
