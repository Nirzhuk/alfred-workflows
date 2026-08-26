pub use super::redaction::redact_text;
use super::redaction::{canonical_key, is_secret_key};
use super::{NativeErrorCode, NativeRuntimeError, NATIVE_EVENT_CONTRACT_VERSION};
use serde::Serialize;
use serde_json::{Map, Value};

pub const DEFAULT_MAX_EVENTS: usize = 2_048;
pub const DEFAULT_MAX_TEXT_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = 128 * 1024;
pub const DEFAULT_MAX_ERROR_BYTES: usize = 8 * 1024;
pub const DEFAULT_MAX_METADATA_BYTES: usize = 16 * 1024;
pub const DEFAULT_MAX_METADATA_DEPTH: usize = 4;
pub const MAX_METADATA_KEYS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEventLimits {
    pub max_events: usize,
    pub max_text_bytes: usize,
    pub max_tool_output_bytes: usize,
    pub max_error_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_metadata_depth: usize,
}

impl Default for NativeEventLimits {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_MAX_EVENTS,
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            max_tool_output_bytes: DEFAULT_MAX_TOOL_OUTPUT_BYTES,
            max_error_bytes: DEFAULT_MAX_ERROR_BYTES,
            max_metadata_bytes: DEFAULT_MAX_METADATA_BYTES,
            max_metadata_depth: DEFAULT_MAX_METADATA_DEPTH,
        }
    }
}

impl NativeEventLimits {
    pub fn validate(&self) -> Result<(), NativeRuntimeError> {
        let valid = self.max_events > 0
            && self.max_events <= DEFAULT_MAX_EVENTS
            && self.max_text_bytes > 0
            && self.max_text_bytes <= DEFAULT_MAX_TEXT_BYTES
            && self.max_tool_output_bytes > 0
            && self.max_tool_output_bytes <= DEFAULT_MAX_TOOL_OUTPUT_BYTES
            && self.max_error_bytes > 0
            && self.max_error_bytes <= DEFAULT_MAX_ERROR_BYTES
            && self.max_metadata_bytes > 0
            && self.max_metadata_bytes <= DEFAULT_MAX_METADATA_BYTES
            && self.max_metadata_depth > 0
            && self.max_metadata_depth <= DEFAULT_MAX_METADATA_DEPTH;
        if valid {
            Ok(())
        } else {
            Err(NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "native event limits exceed the harness maximum",
                false,
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeEventKind {
    SessionStarted,
    TurnStarted,
    AssistantDelta,
    ToolStarted,
    ToolProgress,
    ToolCompleted,
    ApprovalRequested,
    ApprovalResolved,
    Warning,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeContentClass {
    Assistant,
    Reasoning,
}

impl NativeContentClass {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "assistant" => Some(Self::Assistant),
            "reasoning" => Some(Self::Reasoning),
            _ => None,
        }
    }
}

impl NativeEventKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "session_started" => Some(Self::SessionStarted),
            "turn_started" => Some(Self::TurnStarted),
            "assistant_delta" => Some(Self::AssistantDelta),
            "tool_started" => Some(Self::ToolStarted),
            "tool_progress" => Some(Self::ToolProgress),
            "tool_completed" => Some(Self::ToolCompleted),
            "approval_requested" => Some(Self::ApprovalRequested),
            "approval_resolved" => Some(Self::ApprovalResolved),
            "warning" => Some(Self::Warning),
            "turn_completed" => Some(Self::TurnCompleted),
            "turn_failed" => Some(Self::TurnFailed),
            "turn_cancelled" => Some(Self::TurnCancelled),
            _ => None,
        }
    }

    fn allows_text(self) -> bool {
        matches!(
            self,
            Self::AssistantDelta | Self::ToolProgress | Self::Warning
        )
    }

    fn allows_error(self) -> bool {
        matches!(self, Self::TurnFailed | Self::Warning)
    }

    fn is_tool(self) -> bool {
        matches!(
            self,
            Self::ToolStarted | Self::ToolProgress | Self::ToolCompleted
        )
    }

    fn is_approval(self) -> bool {
        matches!(self, Self::ApprovalRequested | Self::ApprovalResolved)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEvent {
    pub contract_version: u16,
    pub sequence: u32,
    pub kind: NativeEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_class: Option<NativeContentClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

impl NativeEvent {
    pub fn new(sequence: u32, kind: NativeEventKind) -> Self {
        Self {
            contract_version: NATIVE_EVENT_CONTRACT_VERSION,
            sequence,
            kind,
            content_class: None,
            session_id: None,
            turn_id: None,
            text: None,
            tool_call_id: None,
            tool_name: None,
            tool_output: None,
            approval_id: None,
            approved: None,
            error: None,
            metadata: Map::new(),
        }
    }
}

pub struct NativeEventNormalizer {
    limits: NativeEventLimits,
    accepted: usize,
    last_sequence: Option<u32>,
}

impl NativeEventNormalizer {
    pub fn new(limits: NativeEventLimits) -> Result<Self, NativeRuntimeError> {
        limits.validate()?;
        Ok(Self {
            limits,
            accepted: 0,
            last_sequence: None,
        })
    }

    pub fn normalize(&mut self, mut event: NativeEvent) -> Result<NativeEvent, NativeRuntimeError> {
        if event.contract_version != NATIVE_EVENT_CONTRACT_VERSION {
            return Err(invalid_event("unsupported native event contract version"));
        }
        if self.accepted >= self.limits.max_events {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::EventLimitExceeded,
                "native event count exceeded the configured limit",
                false,
            ));
        }
        if self
            .last_sequence
            .is_some_and(|last| event.sequence <= last)
        {
            return Err(invalid_event(
                "native event sequence is not strictly increasing",
            ));
        }
        if event.content_class == Some(NativeContentClass::Reasoning) {
            return Err(invalid_event(
                "reasoning content is prohibited in native events",
            ));
        }
        match event.kind {
            NativeEventKind::AssistantDelta
                if event.content_class != Some(NativeContentClass::Assistant) =>
            {
                return Err(invalid_event(
                    "assistant deltas require an explicit assistant content class",
                ));
            }
            NativeEventKind::AssistantDelta => {}
            _ if event.content_class.is_some() => {
                return Err(invalid_event(
                    "content class is only valid for assistant delta events",
                ));
            }
            _ => {}
        }
        validate_optional_id(event.session_id.as_deref(), "session id")?;
        validate_optional_id(event.turn_id.as_deref(), "turn id")?;
        validate_optional_id(event.tool_call_id.as_deref(), "tool call id")?;
        validate_optional_id(event.approval_id.as_deref(), "approval id")?;
        if event.text.is_some() && !event.kind.allows_text() {
            return Err(invalid_event(
                "text is not valid for this native event kind",
            ));
        }
        if event.error.is_some() && !event.kind.allows_error() {
            return Err(invalid_event(
                "error text is not valid for this native event kind",
            ));
        }
        if (event.tool_call_id.is_some()
            || event.tool_name.is_some()
            || event.tool_output.is_some())
            && !event.kind.is_tool()
        {
            return Err(invalid_event(
                "tool fields are not valid for this native event kind",
            ));
        }
        if (event.approval_id.is_some() || event.approved.is_some()) && !event.kind.is_approval() {
            return Err(invalid_event(
                "approval fields are not valid for this native event kind",
            ));
        }
        if let Some(text) = event.text.as_mut() {
            enforce_bytes(text, self.limits.max_text_bytes, "native event text")?;
            *text = redact_text(text);
        }
        if let Some(output) = event.tool_output.as_mut() {
            enforce_bytes(
                output,
                self.limits.max_tool_output_bytes,
                "native tool output",
            )?;
            *output = redact_text(output);
        }
        if let Some(error) = event.error.as_mut() {
            enforce_bytes(error, self.limits.max_error_bytes, "native error text")?;
            *error = redact_text(error);
        }
        if let Some(name) = event.tool_name.as_mut() {
            enforce_bytes(name, 128, "native tool name")?;
            *name = redact_text(name);
        }
        event.metadata = sanitize_metadata(Value::Object(event.metadata), &self.limits)?
            .as_object()
            .cloned()
            .unwrap_or_default();
        self.accepted += 1;
        self.last_sequence = Some(event.sequence);
        Ok(event)
    }

    pub fn normalize_untrusted(&mut self, value: Value) -> Result<NativeEvent, NativeRuntimeError> {
        let object = value
            .as_object()
            .ok_or_else(|| invalid_event("native event must be an object"))?;
        if contains_reasoning_field(&value) {
            return Err(invalid_event(
                "reasoning content is prohibited in native events",
            ));
        }
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "contractVersion"
                    | "sequence"
                    | "kind"
                    | "contentClass"
                    | "sessionId"
                    | "turnId"
                    | "text"
                    | "toolCallId"
                    | "toolName"
                    | "toolOutput"
                    | "approvalId"
                    | "approved"
                    | "error"
                    | "metadata"
            ) {
                return Err(invalid_event("native event contains an unknown core field"));
            }
        }
        let contract_version = object
            .get("contractVersion")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| invalid_event("native event contract version is missing"))?;
        let sequence = object
            .get("sequence")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| invalid_event("native event sequence is invalid"))?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .and_then(NativeEventKind::parse)
            .ok_or_else(|| invalid_event("native event kind is invalid"))?;
        let content_class = match object.get("contentClass") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(
                NativeContentClass::parse(value)
                    .ok_or_else(|| invalid_event("native event content class is invalid"))?,
            ),
            Some(_) => return Err(invalid_event("native event content class must be a string")),
        };
        let metadata = match object.get("metadata") {
            None => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Err(invalid_event("native event metadata must be an object")),
        };
        self.normalize(NativeEvent {
            contract_version,
            sequence,
            kind,
            content_class,
            session_id: optional_string(object, "sessionId")?,
            turn_id: optional_string(object, "turnId")?,
            text: optional_string(object, "text")?,
            tool_call_id: optional_string(object, "toolCallId")?,
            tool_name: optional_string(object, "toolName")?,
            tool_output: optional_string(object, "toolOutput")?,
            approval_id: optional_string(object, "approvalId")?,
            approved: optional_bool(object, "approved")?,
            error: optional_string(object, "error")?,
            metadata,
        })
    }
}

fn optional_string(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, NativeRuntimeError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_event(format!(
            "native event {key} must be a string"
        ))),
    }
}

fn optional_bool(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<bool>, NativeRuntimeError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(invalid_event(format!(
            "native event {key} must be a boolean"
        ))),
    }
}

fn validate_optional_id(value: Option<&str>, label: &str) -> Result<(), NativeRuntimeError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(invalid_event(format!("{label} is invalid")));
    }
    Ok(())
}

fn enforce_bytes(value: &str, maximum: usize, label: &str) -> Result<(), NativeRuntimeError> {
    if value.len() > maximum {
        Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            format!("{label} exceeded its byte limit"),
            false,
        ))
    } else {
        Ok(())
    }
}

fn invalid_event(message: impl Into<String>) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidEvent, message, false)
}

fn contains_reasoning_field(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, child)| is_reasoning_key(key) || contains_reasoning_field(child)),
        Value::Array(values) => values.iter().any(contains_reasoning_field),
        _ => false,
    }
}

fn is_reasoning_key(key: &str) -> bool {
    matches!(
        canonical_key(key).as_str(),
        "reasoning" | "chainofthought" | "thinking" | "thoughts" | "reasoningcontent"
    )
}

fn sanitize_metadata(
    value: Value,
    limits: &NativeEventLimits,
) -> Result<Value, NativeRuntimeError> {
    if contains_reasoning_field(&value) {
        return Err(invalid_event(
            "reasoning content is prohibited in native metadata",
        ));
    }
    let sanitized = sanitize_value(value, 0, limits)?;
    let serialized = serde_json::to_vec(&sanitized)
        .map_err(|_| invalid_event("native metadata could not be serialized"))?;
    if serialized.len() > limits.max_metadata_bytes {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "native event metadata exceeded its byte limit",
            false,
        ));
    }
    Ok(sanitized)
}

fn sanitize_value(
    value: Value,
    depth: usize,
    limits: &NativeEventLimits,
) -> Result<Value, NativeRuntimeError> {
    if depth >= limits.max_metadata_depth {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "native event metadata exceeded its depth limit",
            false,
        ));
    }
    match value {
        Value::Object(map) => {
            if map.len() > MAX_METADATA_KEYS {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "native event metadata contains too many keys",
                    false,
                ));
            }
            let mut output = Map::new();
            for (key, child) in map {
                if is_secret_key(&key) {
                    output.insert(key, Value::String("[REDACTED]".into()));
                } else {
                    output.insert(key, sanitize_value(child, depth + 1, limits)?);
                }
            }
            Ok(Value::Object(output))
        }
        Value::Array(values) => {
            if values.len() > MAX_METADATA_KEYS {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "native event metadata array is too large",
                    false,
                ));
            }
            values
                .into_iter()
                .map(|child| sanitize_value(child, depth + 1, limits))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        Value::String(value) => Ok(Value::String(redact_text(&value))),
        scalar => Ok(scalar),
    }
}
