//! Pinned OpenCode 1.18.23 V1 request/event mapping.
//!
//! No `/experimental/*`, `/api/*` V2, custom-tool, or raw method surface is
//! represented here.

use super::account::OPENCODE_GO_PROVIDER_ID;
use crate::agents::native::{
    contains_secret_marker, redact_text, NativeApprovalPolicy, NativeContentClass,
    NativeContextRole, NativeErrorCode, NativeEvent, NativeEventKind, NativeModel,
    NativeRuntimeError, NativeTurnRequest,
};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const MAX_SSE_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_SSE_FRAME_BYTES: usize = 256 * 1024;
const MAX_SSE_EVENTS: usize = 16_384;
const MAX_WIRE_ID_BYTES: usize = 128;
const MAX_MODEL_ID_BYTES: usize = 224;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_PERMISSION_PATTERNS: usize = 64;
const MAX_PERMISSION_PATTERN_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeRoute {
    model_id: String,
}

impl OpenCodeRoute {
    pub fn parse(value: &str) -> Result<Self, NativeRuntimeError> {
        let (provider_id, model_id) = value.split_once('/').ok_or_else(model_unavailable)?;
        if provider_id != OPENCODE_GO_PROVIDER_ID || !valid_model_id(model_id) {
            return Err(model_unavailable());
        }
        Ok(Self {
            model_id: model_id.into(),
        })
    }

    pub fn provider_id(&self) -> &'static str {
        OPENCODE_GO_PROVIDER_ID
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn id(&self) -> String {
        format!("{OPENCODE_GO_PROVIDER_ID}/{}", self.model_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodePermissionReply {
    Once,
    Always,
    Reject,
}

impl OpenCodePermissionReply {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }

    pub fn approved(self) -> bool {
        !matches!(self, Self::Reject)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodePermissionRequest {
    pub request_id: String,
    pub session_id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub always_patterns: Vec<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodeGoFailure {
    Account,
    RateLimited,
    OutputLimited,
    Aborted,
    Provider,
}

impl OpenCodeGoFailure {
    pub fn error(self) -> NativeRuntimeError {
        match self {
            Self::Account => NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "OpenCode Go account is unavailable; reconnect the Go key",
                false,
            ),
            Self::RateLimited => NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "OpenCode Go usage limit reached; usage is available only in the OpenCode console",
                true,
            ),
            Self::OutputLimited => NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "OpenCode Go stopped at the model output limit",
                true,
            ),
            Self::Aborted => NativeRuntimeError::cancelled(),
            Self::Provider => NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "OpenCode Go could not complete the turn",
                true,
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenCodeProtocolEvent {
    Connected,
    AssistantDelta(NativeEvent),
    ToolEvent(NativeEvent),
    PermissionAsked(OpenCodePermissionRequest),
    PermissionReplied {
        request_id: String,
        session_id: String,
        reply: OpenCodePermissionReply,
    },
    SessionBusy,
    SessionRetry,
    SessionIdle,
    SessionError(OpenCodeGoFailure),
    Ignored,
}

pub fn parse_go_models(value: &Value) -> Result<Vec<NativeModel>, NativeRuntimeError> {
    let object = value.as_object().ok_or_else(protocol_error)?;
    let connected = object
        .get("connected")
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?;
    if !connected
        .iter()
        .any(|provider| provider.as_str() == Some(OPENCODE_GO_PROVIDER_ID))
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::AccountUnavailable,
            "OpenCode Go provider is not connected",
            false,
        ));
    }
    let all = object
        .get("all")
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?;
    let mut matches = all.iter().filter(|provider| {
        provider.get("id").and_then(Value::as_str) == Some(OPENCODE_GO_PROVIDER_ID)
    });
    let provider = matches.next().ok_or_else(model_unavailable)?;
    if matches.next().is_some() {
        return Err(protocol_error());
    }
    let models = provider
        .get("models")
        .and_then(Value::as_object)
        .ok_or_else(model_unavailable)?;
    if models.is_empty() || models.len() > 512 {
        return Err(model_unavailable());
    }
    let mut result = Vec::with_capacity(models.len());
    for (id, model) in models {
        if !valid_model_id(id) {
            return Err(model_unavailable());
        }
        if model.get("id").and_then(Value::as_str) != Some(id.as_str())
            || model.get("providerID").and_then(Value::as_str) != Some(OPENCODE_GO_PROVIDER_ID)
        {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "OpenCode model catalog attempted another provider route",
                false,
            ));
        }
        let label = model.get("name").and_then(Value::as_str).unwrap_or(id);
        if label.is_empty()
            || label.len() > 256
            || label.chars().any(char::is_control)
            || contains_secret_marker(label)
        {
            return Err(model_unavailable());
        }
        result.push(NativeModel {
            id: format!("{OPENCODE_GO_PROVIDER_ID}/{id}"),
            label: redact_text(label),
        });
    }
    result.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(result)
}

pub fn session_id(value: &Value, expected_directory: &str) -> Result<String, NativeRuntimeError> {
    let object = value.as_object().ok_or_else(protocol_error)?;
    let id = wire_id(required_string(object, "id")?, "session id")?;
    let directory = required_string(object, "directory")?;
    if directory != expected_directory {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::SessionUnavailable,
            "OpenCode session belongs to another repository",
            false,
        ));
    }
    Ok(id)
}

pub fn build_session_body(request: &NativeTurnRequest, _route: &OpenCodeRoute) -> Value {
    json!({
        "title": "Alfred managed session",
        "permission": permission_rules(request),
    })
}

pub fn build_prompt_body(request: &NativeTurnRequest, route: &OpenCodeRoute) -> Value {
    let system = request
        .context
        .iter()
        .filter(|block| {
            matches!(
                block.role,
                NativeContextRole::System | NativeContextRole::Skill
            )
        })
        .map(|block| block.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut body = json!({
        "model": {"providerID": route.provider_id(), "modelID": route.model_id()},
        "parts": [{"type": "text", "text": request.prompt}],
    });
    if !system.is_empty() {
        body["system"] = Value::String(system);
    }
    body
}

pub fn permission_policy(permission: &str, request: &NativeTurnRequest) -> NativeApprovalPolicy {
    match permission {
        "read" | "glob" | "grep" | "list" if request.tool_capabilities.filesystem => {
            request.permission_profile.filesystem
        }
        "edit" | "write" | "apply_patch" if request.tool_capabilities.patch => {
            request.permission_profile.filesystem
        }
        "bash" if request.tool_capabilities.shell => request.permission_profile.shell,
        "task" if request.tool_capabilities.subagents => request.permission_profile.subagents,
        _ => NativeApprovalPolicy::Deny,
    }
}

fn permission_rules(request: &NativeTurnRequest) -> Vec<Value> {
    let mut rules = vec![permission_rule("*", "deny")];
    if request.tool_capabilities.filesystem {
        for permission in ["read", "glob", "grep", "list"] {
            push_policy_rule(
                &mut rules,
                permission,
                request.permission_profile.filesystem,
            );
        }
    }
    if request.tool_capabilities.patch {
        for permission in ["edit", "write", "apply_patch"] {
            push_policy_rule(
                &mut rules,
                permission,
                request.permission_profile.filesystem,
            );
        }
    }
    if request.tool_capabilities.shell {
        push_policy_rule(&mut rules, "bash", request.permission_profile.shell);
    }
    if request.tool_capabilities.subagents {
        push_policy_rule(&mut rules, "task", request.permission_profile.subagents);
    }
    // OpenCode's profile HOME is isolated, but built-in tools still may not
    // escape Alfred's explicitly selected repository.
    rules.push(json!({
        "permission": "external_directory",
        "pattern": "*",
        "action": "deny"
    }));
    rules
}

fn push_policy_rule(rules: &mut Vec<Value>, permission: &str, policy: NativeApprovalPolicy) {
    let action = match policy {
        NativeApprovalPolicy::Deny => "deny",
        NativeApprovalPolicy::Ask => "ask",
        NativeApprovalPolicy::Allow => "allow",
    };
    rules.push(permission_rule(permission, action));
}

fn permission_rule(permission: &str, action: &str) -> Value {
    json!({"permission": permission, "pattern": "*", "action": action})
}

#[derive(Default)]
pub struct OpenCodeEventMapper {
    expected_session_id: String,
    text_parts: HashMap<String, String>,
    tool_states: HashMap<String, String>,
}

impl OpenCodeEventMapper {
    pub fn new(expected_session_id: String) -> Result<Self, NativeRuntimeError> {
        wire_id(&expected_session_id, "session id")?;
        Ok(Self {
            expected_session_id,
            text_parts: HashMap::new(),
            tool_states: HashMap::new(),
        })
    }

    pub fn map(&mut self, value: Value) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        let encoded = serde_json::to_vec(&value).map_err(|_| protocol_error())?;
        if encoded.len() > MAX_SSE_FRAME_BYTES {
            return Err(limit_error());
        }
        let object = value.as_object().ok_or_else(protocol_error)?;
        let event_type = required_string(object, "type")?;
        let properties = object
            .get("properties")
            .and_then(Value::as_object)
            .ok_or_else(protocol_error)?;
        match event_type {
            "server.connected" => Ok(OpenCodeProtocolEvent::Connected),
            "message.part.updated" => self.map_part(properties),
            "permission.asked" => self.map_permission(properties),
            "permission.replied" => self.map_permission_reply(properties),
            "session.status" => self.map_status(properties),
            "session.idle" => {
                self.require_session(properties)?;
                Ok(OpenCodeProtocolEvent::SessionIdle)
            }
            "session.error" => self.map_error(properties),
            // V2 and custom-tool events are deliberately not decoded.
            event
                if event.starts_with("session.next.")
                    || event.starts_with("permission.v2.")
                    || event.starts_with("tool.") =>
            {
                Ok(OpenCodeProtocolEvent::Ignored)
            }
            _ => Ok(OpenCodeProtocolEvent::Ignored),
        }
    }

    fn map_part(
        &mut self,
        properties: &Map<String, Value>,
    ) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        let part = properties
            .get("part")
            .and_then(Value::as_object)
            .ok_or_else(protocol_error)?;
        let part_session = wire_id(required_string(part, "sessionID")?, "session id")?;
        self.validate_session(&part_session)?;
        match required_string(part, "type")? {
            "text" => {
                let part_id = wire_id(required_string(part, "id")?, "part id")?;
                let text = required_string(part, "text")?;
                if text.len() > MAX_TEXT_BYTES {
                    return Err(limit_error());
                }
                let previous = self
                    .text_parts
                    .get(&part_id)
                    .map(String::as_str)
                    .unwrap_or("");
                let delta = text.strip_prefix(previous).ok_or_else(protocol_error)?;
                self.text_parts.insert(part_id, text.into());
                if delta.is_empty() {
                    return Ok(OpenCodeProtocolEvent::Ignored);
                }
                let mut event = NativeEvent::new(0, NativeEventKind::AssistantDelta);
                event.content_class = Some(NativeContentClass::Assistant);
                event.session_id = Some(self.expected_session_id.clone());
                event.text = Some(redact_text(delta));
                Ok(OpenCodeProtocolEvent::AssistantDelta(event))
            }
            "reasoning" => Ok(OpenCodeProtocolEvent::Ignored),
            "tool" => self.map_tool(part),
            _ => Ok(OpenCodeProtocolEvent::Ignored),
        }
    }

    fn map_tool(
        &mut self,
        part: &Map<String, Value>,
    ) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        let call_id = wire_id(required_string(part, "callID")?, "tool call id")?;
        let tool = bounded(required_string(part, "tool")?, 128, "tool name")?;
        let state = part
            .get("state")
            .and_then(Value::as_object)
            .ok_or_else(protocol_error)?;
        let status = required_string(state, "status")?;
        if self.tool_states.get(&call_id).map(String::as_str) == Some(status) {
            return Ok(OpenCodeProtocolEvent::Ignored);
        }
        self.tool_states.insert(call_id.clone(), status.into());
        let mut event = match status {
            "pending" => return Ok(OpenCodeProtocolEvent::Ignored),
            "running" => NativeEvent::new(0, NativeEventKind::ToolStarted),
            "completed" | "error" => NativeEvent::new(0, NativeEventKind::ToolCompleted),
            _ => return Err(protocol_error()),
        };
        event.session_id = Some(self.expected_session_id.clone());
        event.tool_call_id = Some(call_id);
        event.tool_name = Some(redact_text(&tool));
        if status == "completed" {
            let output = bounded(
                required_string(state, "output")?,
                MAX_TEXT_BYTES,
                "tool output",
            )?;
            event.tool_output = Some(redact_text(&output));
        } else if status == "error" {
            event.tool_output = Some("OpenCode tool failed.".into());
        }
        Ok(OpenCodeProtocolEvent::ToolEvent(event))
    }

    fn map_permission(
        &self,
        properties: &Map<String, Value>,
    ) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        self.require_session(properties)?;
        let request_id = wire_id(required_string(properties, "id")?, "permission id")?;
        let permission = bounded(
            required_string(properties, "permission")?,
            128,
            "permission name",
        )?;
        let patterns = permission_patterns(properties, "patterns")?;
        let always_patterns = permission_patterns(properties, "always")?;
        let tool_call_id = properties
            .get("tool")
            .and_then(Value::as_object)
            .and_then(|tool| tool.get("callID"))
            .and_then(Value::as_str)
            .map(|id| wire_id(id, "tool call id"))
            .transpose()?;
        Ok(OpenCodeProtocolEvent::PermissionAsked(
            OpenCodePermissionRequest {
                request_id,
                session_id: self.expected_session_id.clone(),
                permission,
                patterns,
                always_patterns,
                tool_call_id,
            },
        ))
    }

    fn map_permission_reply(
        &self,
        properties: &Map<String, Value>,
    ) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        self.require_session(properties)?;
        let request_id = wire_id(required_string(properties, "requestID")?, "permission id")?;
        let reply = match required_string(properties, "reply")? {
            "once" => OpenCodePermissionReply::Once,
            "always" => OpenCodePermissionReply::Always,
            "reject" => OpenCodePermissionReply::Reject,
            _ => return Err(protocol_error()),
        };
        Ok(OpenCodeProtocolEvent::PermissionReplied {
            request_id,
            session_id: self.expected_session_id.clone(),
            reply,
        })
    }

    fn map_status(
        &self,
        properties: &Map<String, Value>,
    ) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        self.require_session(properties)?;
        let status = properties
            .get("status")
            .and_then(Value::as_object)
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            .ok_or_else(protocol_error)?;
        match status {
            "busy" => Ok(OpenCodeProtocolEvent::SessionBusy),
            "retry" => Ok(OpenCodeProtocolEvent::SessionRetry),
            "idle" => Ok(OpenCodeProtocolEvent::SessionIdle),
            _ => Err(protocol_error()),
        }
    }

    fn map_error(
        &self,
        properties: &Map<String, Value>,
    ) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
        if let Some(session) = properties.get("sessionID").and_then(Value::as_str) {
            self.validate_session(session)?;
        }
        let Some(error) = properties.get("error").and_then(Value::as_object) else {
            return Ok(OpenCodeProtocolEvent::SessionError(
                OpenCodeGoFailure::Provider,
            ));
        };
        let name = required_string(error, "name")?;
        let data = error
            .get("data")
            .and_then(Value::as_object)
            .ok_or_else(protocol_error)?;
        let failure = match name {
            "ProviderAuthError" => {
                if required_string(data, "providerID")? != OPENCODE_GO_PROVIDER_ID {
                    return Err(NativeRuntimeError::new(
                        NativeErrorCode::AccountMismatch,
                        "OpenCode event attempted another provider route",
                        false,
                    ));
                }
                OpenCodeGoFailure::Account
            }
            "MessageOutputLengthError" => OpenCodeGoFailure::OutputLimited,
            "MessageAbortedError" => OpenCodeGoFailure::Aborted,
            "APIError" => classify_api_error(data),
            _ => OpenCodeGoFailure::Provider,
        };
        Ok(OpenCodeProtocolEvent::SessionError(failure))
    }

    fn require_session(&self, properties: &Map<String, Value>) -> Result<(), NativeRuntimeError> {
        self.validate_session(required_string(properties, "sessionID")?)
    }

    fn validate_session(&self, actual: &str) -> Result<(), NativeRuntimeError> {
        if actual == self.expected_session_id {
            Ok(())
        } else {
            Err(NativeRuntimeError::new(
                NativeErrorCode::SessionUnavailable,
                "OpenCode event belongs to another session",
                false,
            ))
        }
    }
}

fn permission_patterns(
    properties: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, NativeRuntimeError> {
    let raw = properties
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(protocol_error)?;
    if raw.len() > MAX_PERMISSION_PATTERNS {
        return Err(limit_error());
    }
    raw.iter()
        .map(|value| {
            let pattern = value.as_str().ok_or_else(protocol_error)?;
            bounded(pattern, MAX_PERMISSION_PATTERN_BYTES, "permission pattern")
                .map(|pattern| redact_text(&pattern))
        })
        .collect()
}

fn classify_api_error(data: &Map<String, Value>) -> OpenCodeGoFailure {
    let status = data.get("statusCode").and_then(Value::as_u64);
    if matches!(status, Some(402) | Some(429)) {
        return OpenCodeGoFailure::RateLimited;
    }
    let message = data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if [
        "rate limit",
        "usage limit",
        "quota exceeded",
        "limit reached",
    ]
    .iter()
    .any(|marker| message.contains(marker))
    {
        OpenCodeGoFailure::RateLimited
    } else if matches!(status, Some(401) | Some(403)) {
        OpenCodeGoFailure::Account
    } else {
        OpenCodeGoFailure::Provider
    }
}

#[derive(Default)]
pub struct OpenCodeSseDecoder {
    buffer: Vec<u8>,
    events: usize,
}

impl OpenCodeSseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Value>, NativeRuntimeError> {
        if self.buffer.len().saturating_add(chunk.len()) > MAX_SSE_BUFFER_BYTES {
            return Err(limit_error());
        }
        self.buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        loop {
            let Some((end, delimiter_len)) = find_frame_end(&self.buffer) else {
                break;
            };
            if end > MAX_SSE_FRAME_BYTES {
                return Err(limit_error());
            }
            let frame = self.buffer.drain(..end + delimiter_len).collect::<Vec<_>>();
            let payload = data_payload(&frame[..end])?;
            if payload.is_empty() {
                continue;
            }
            self.events = self.events.saturating_add(1);
            if self.events > MAX_SSE_EVENTS {
                return Err(limit_error());
            }
            output.push(serde_json::from_slice(&payload).map_err(|_| protocol_error())?);
        }
        Ok(output)
    }

    pub fn finish(&self) -> Result<(), NativeRuntimeError> {
        if self.buffer.iter().all(u8::is_ascii_whitespace) {
            Ok(())
        } else {
            Err(protocol_error())
        }
    }
}

fn find_frame_end(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(found), None) | (None, Some(found)) => Some(found),
        (None, None) => None,
    }
}

fn data_payload(frame: &[u8]) -> Result<Vec<u8>, NativeRuntimeError> {
    let text = std::str::from_utf8(frame).map_err(|_| protocol_error())?;
    let mut payload = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
            continue;
        }
        let Some(value) = line.strip_prefix("data:") else {
            return Err(protocol_error());
        };
        if !payload.is_empty() {
            payload.push(b'\n');
        }
        payload.extend_from_slice(value.strip_prefix(' ').unwrap_or(value).as_bytes());
    }
    Ok(payload)
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, NativeRuntimeError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(protocol_error)
}

fn wire_id(value: &str, label: &str) -> Result<String, NativeRuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_WIRE_ID_BYTES
        && !contains_secret_marker(value)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if valid {
        Ok(value.into())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidEvent,
            format!("OpenCode {label} is invalid"),
            false,
        ))
    }
}

fn bounded(value: &str, maximum: usize, label: &str) -> Result<String, NativeRuntimeError> {
    if value.is_empty() || value.len() > maximum || value.chars().any(|character| character == '\0')
    {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidEvent,
            format!("OpenCode {label} is invalid"),
            false,
        ))
    } else {
        Ok(value.into())
    }
}

fn valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_MODEL_ID_BYTES
        && value.trim() == value
        && !contains_secret_marker(value)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
}

fn model_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ModelUnavailable,
        "OpenCode Go model route is unavailable",
        false,
    )
}

fn protocol_error() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidEvent,
        "OpenCode server event did not match the pinned V1 contract",
        false,
    )
}

fn limit_error() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::EventLimitExceeded,
        "OpenCode server stream exceeded its bounded limit",
        false,
    )
}
