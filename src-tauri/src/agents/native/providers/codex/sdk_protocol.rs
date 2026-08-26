//! Bounded JSONL protocol between Rust and the managed Python SDK sidecar.
//!
//! Only sidecar-owned method names and token-free result DTOs are accepted.
//! The Python process, not Rust, is the sole consumer of the public Codex SDK.

use crate::agents::native::{
    contains_secret_marker, NativeContentClass, NativeErrorCode, NativeEvent, NativeEventKind,
    NativeEventLimits, NativeEventNormalizer, NativeRuntimeError,
};
use serde::de::{DeserializeOwned, Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use url::Url;

pub const CODEX_SDK_PROTOCOL_VERSION: u16 = 1;
pub const MAX_CODEX_SDK_FRAME_BYTES: usize = 256 * 1024;
pub const MAX_CODEX_SDK_PENDING_REQUESTS: usize = 64;
pub const MAX_CODEX_SDK_OPERATIONS: usize = 64;
pub const CODEX_SDK_HOST_APPROVAL_BLOCKER: &str = "codex_python_sdk_host_approval_unavailable";

const MAX_ID_BYTES: usize = 256;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MAX_LOGIN_ID_BYTES: usize = 128;
const MAX_MODEL_BYTES: usize = 256;
const MAX_PROMPT_BYTES: usize = 128 * 1024;
const MAX_CWD_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSdkMethod {
    Capabilities,
    LoginStart,
    LoginWait,
    LoginCancel,
    Account,
    Logout,
    Models,
    ThreadStart,
    ThreadResume,
    TurnStart,
    TurnCancel,
    ApprovalDecide,
    Shutdown,
}

impl CodexSdkMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::LoginStart => "login_start",
            Self::LoginWait => "login_wait",
            Self::LoginCancel => "login_cancel",
            Self::Account => "account",
            Self::Logout => "logout",
            Self::Models => "models",
            Self::ThreadStart => "thread_start",
            Self::ThreadResume => "thread_resume",
            Self::TurnStart => "turn_start",
            Self::TurnCancel => "turn_cancel",
            Self::ApprovalDecide => "approval_decide",
            Self::Shutdown => "shutdown",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "capabilities" => Self::Capabilities,
            "login_start" => Self::LoginStart,
            "login_wait" => Self::LoginWait,
            "login_cancel" => Self::LoginCancel,
            "account" => Self::Account,
            "logout" => Self::Logout,
            "models" => Self::Models,
            "thread_start" => Self::ThreadStart,
            "thread_resume" => Self::ThreadResume,
            "turn_start" => Self::TurnStart,
            "turn_cancel" => Self::TurnCancel,
            "approval_decide" => Self::ApprovalDecide,
            "shutdown" => Self::Shutdown,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSdkLoginKind {
    Browser,
    DeviceCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSdkApprovalDecision {
    Allow,
    Deny,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RequestFrame<'a> {
    protocol_version: u16,
    request_id: &'a str,
    method: &'static str,
    params: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkReady {
    #[serde(rename = "type")]
    pub frame_type: ReadyFrameType,
    pub protocol_version: u16,
    pub sdk_version: String,
    pub experimental_api: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyFrameType {
    Ready,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkResponse {
    #[serde(rename = "type")]
    _frame_type: ResponseFrameType,
    pub protocol_version: u16,
    pub request_id: String,
    pub method: String,
    result: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResponseFrameType {
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkErrorFrame {
    #[serde(rename = "type")]
    _frame_type: ErrorFrameType,
    pub protocol_version: u16,
    pub request_id: Option<String>,
    pub code: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ErrorFrameType {
    Error,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct EventFrame {
    #[serde(rename = "type")]
    _frame_type: EventFrameType,
    protocol_version: u16,
    operation_id: String,
    event: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EventFrameType {
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexSdkStreamEvent {
    LoginCompleted {
        login_id: String,
        success: bool,
    },
    TurnStarted {
        thread_id: String,
        turn_id: String,
    },
    AssistantDelta {
        thread_id: String,
        turn_id: String,
        text: String,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
        status: CodexSdkTurnStatus,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSdkTurnStatus {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexSdkInbound {
    Ready(CodexSdkReady),
    Response(CodexSdkResponse),
    Event {
        operation_id: String,
        event: CodexSdkStreamEvent,
    },
    Error(CodexSdkErrorFrame),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSdkProtocolErrorCode {
    MalformedFrame,
    OversizedFrame,
    InvalidField,
    ProtocolMismatch,
    RequestQueueFull,
    OperationQueueFull,
    UnknownRequest,
    UnknownOperation,
    MethodMismatch,
    InvalidResult,
    EncodeFailed,
}

impl CodexSdkProtocolErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MalformedFrame => "codex_sidecar_malformed_frame",
            Self::OversizedFrame => "codex_sidecar_frame_too_large",
            Self::InvalidField => "codex_sidecar_invalid_field",
            Self::ProtocolMismatch => "codex_sidecar_protocol_mismatch",
            Self::RequestQueueFull => "codex_sidecar_request_limit_exceeded",
            Self::OperationQueueFull => "codex_sidecar_operation_limit_exceeded",
            Self::UnknownRequest => "codex_sidecar_unknown_request",
            Self::UnknownOperation => "codex_sidecar_unknown_operation",
            Self::MethodMismatch => "codex_sidecar_method_mismatch",
            Self::InvalidResult => "codex_sidecar_invalid_result",
            Self::EncodeFailed => "codex_sidecar_encode_failed",
        }
    }
}

pub struct CodexSdkProtocolError(CodexSdkProtocolErrorCode);

impl CodexSdkProtocolError {
    pub fn code(&self) -> CodexSdkProtocolErrorCode {
        self.0
    }
}

impl fmt::Debug for CodexSdkProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl fmt::Display for CodexSdkProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

impl std::error::Error for CodexSdkProtocolError {}

pub type ProtocolResult<T> = Result<T, CodexSdkProtocolError>;

fn protocol_error(code: CodexSdkProtocolErrorCode) -> CodexSdkProtocolError {
    CodexSdkProtocolError(code)
}

#[derive(Default)]
pub struct CodexSdkProtocol {
    pending: BTreeMap<String, CodexSdkMethod>,
    operations: BTreeMap<String, ExpectedOperation>,
}

#[derive(Clone, PartialEq, Eq)]
enum ExpectedOperation {
    Login { login_id: String },
    Turn { thread_id: String, turn_id: String },
}

impl CodexSdkProtocol {
    pub fn encode_request(
        &mut self,
        request_id: &str,
        method: CodexSdkMethod,
        params: Value,
    ) -> ProtocolResult<Vec<u8>> {
        validate_request_id(request_id)?;
        if !params.is_object() {
            return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField));
        }
        if self.pending.len() >= MAX_CODEX_SDK_PENDING_REQUESTS
            || self.pending.contains_key(request_id)
        {
            return Err(protocol_error(CodexSdkProtocolErrorCode::RequestQueueFull));
        }
        let bytes = serde_json::to_vec(&RequestFrame {
            protocol_version: CODEX_SDK_PROTOCOL_VERSION,
            request_id,
            method: method.as_str(),
            params,
        })
        .map_err(|_| protocol_error(CodexSdkProtocolErrorCode::EncodeFailed))?;
        validate_frame_bytes(&bytes)?;
        self.pending.insert(request_id.to_owned(), method);
        Ok(bytes)
    }

    pub fn track_login_operation(&mut self, login_id: &str) -> ProtocolResult<()> {
        validate_id(login_id, MAX_LOGIN_ID_BYTES)?;
        self.insert_operation(
            login_id,
            ExpectedOperation::Login {
                login_id: login_id.into(),
            },
        )
    }

    pub fn track_turn_operation(
        &mut self,
        operation_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> ProtocolResult<()> {
        validate_request_id(operation_id)?;
        validate_id(thread_id, MAX_ID_BYTES)?;
        validate_id(turn_id, MAX_ID_BYTES)?;
        self.insert_operation(
            operation_id,
            ExpectedOperation::Turn {
                thread_id: thread_id.into(),
                turn_id: turn_id.into(),
            },
        )
    }

    fn insert_operation(
        &mut self,
        operation_id: &str,
        expected: ExpectedOperation,
    ) -> ProtocolResult<()> {
        if self.operations.len() >= MAX_CODEX_SDK_OPERATIONS
            || self.operations.contains_key(operation_id)
        {
            return Err(protocol_error(
                CodexSdkProtocolErrorCode::OperationQueueFull,
            ));
        }
        self.operations.insert(operation_id.to_owned(), expected);
        Ok(())
    }

    pub fn ingest(&mut self, bytes: &[u8]) -> ProtocolResult<CodexSdkInbound> {
        validate_frame_bytes(bytes)?;
        let value = serde_json::from_slice::<StrictJsonValue>(bytes)
            .map_err(|_| protocol_error(CodexSdkProtocolErrorCode::MalformedFrame))?
            .0;
        let frame_type = value
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            .ok_or_else(|| protocol_error(CodexSdkProtocolErrorCode::MalformedFrame))?;
        match frame_type {
            "ready" => {
                let ready: CodexSdkReady = decode(value)?;
                if ready.protocol_version != CODEX_SDK_PROTOCOL_VERSION
                    || ready.sdk_version != super::CODEX_SDK_RUNTIME_VERSION
                    || ready.experimental_api
                {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::ProtocolMismatch));
                }
                Ok(CodexSdkInbound::Ready(ready))
            }
            "response" => {
                let response: CodexSdkResponse = decode(value)?;
                validate_request_id(&response.request_id)?;
                if response.protocol_version != CODEX_SDK_PROTOCOL_VERSION {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::ProtocolMismatch));
                }
                let method = CodexSdkMethod::parse(&response.method)
                    .ok_or_else(|| protocol_error(CodexSdkProtocolErrorCode::MethodMismatch))?;
                let pending = self
                    .pending
                    .remove(&response.request_id)
                    .ok_or_else(|| protocol_error(CodexSdkProtocolErrorCode::UnknownRequest))?;
                if pending != method {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::MethodMismatch));
                }
                Ok(CodexSdkInbound::Response(response))
            }
            "event" => {
                let frame: EventFrame = decode(value)?;
                validate_id(&frame.operation_id, MAX_ID_BYTES)?;
                if frame.protocol_version != CODEX_SDK_PROTOCOL_VERSION {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::ProtocolMismatch));
                }
                let event = parse_event(frame.event)?;
                let expected = self
                    .operations
                    .get(&frame.operation_id)
                    .ok_or_else(|| protocol_error(CodexSdkProtocolErrorCode::UnknownOperation))?;
                if !operation_matches(expected, &event) {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField));
                }
                if matches!(
                    &event,
                    CodexSdkStreamEvent::LoginCompleted { .. }
                        | CodexSdkStreamEvent::TurnCompleted { .. }
                ) {
                    self.operations.remove(&frame.operation_id);
                }
                Ok(CodexSdkInbound::Event {
                    operation_id: frame.operation_id,
                    event,
                })
            }
            "error" => {
                let error: CodexSdkErrorFrame = decode(value)?;
                if error.protocol_version != CODEX_SDK_PROTOCOL_VERSION
                    || !valid_error_code(&error.code)
                {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::ProtocolMismatch));
                }
                let request_id = error
                    .request_id
                    .as_deref()
                    .ok_or_else(|| protocol_error(CodexSdkProtocolErrorCode::UnknownRequest))?;
                validate_id(request_id, MAX_ID_BYTES)?;
                if self.pending.remove(request_id).is_none()
                    && self.operations.remove(request_id).is_none()
                {
                    return Err(protocol_error(CodexSdkProtocolErrorCode::UnknownRequest));
                }
                Ok(CodexSdkInbound::Error(error))
            }
            _ => Err(protocol_error(CodexSdkProtocolErrorCode::MalformedFrame)),
        }
    }

    pub fn process_exited(&mut self) {
        self.pending.clear();
        self.operations.clear();
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn operation_len(&self) -> usize {
        self.operations.len()
    }
}

fn operation_matches(expected: &ExpectedOperation, event: &CodexSdkStreamEvent) -> bool {
    match (expected, event) {
        (
            ExpectedOperation::Login { login_id: expected },
            CodexSdkStreamEvent::LoginCompleted { login_id, .. },
        ) => expected == login_id,
        (
            ExpectedOperation::Turn {
                thread_id: expected_thread,
                turn_id: expected_turn,
            },
            CodexSdkStreamEvent::TurnStarted { thread_id, turn_id }
            | CodexSdkStreamEvent::AssistantDelta {
                thread_id, turn_id, ..
            }
            | CodexSdkStreamEvent::TurnCompleted {
                thread_id, turn_id, ..
            },
        ) => expected_thread == thread_id && expected_turn == turn_id,
        _ => false,
    }
}

fn decode<T: DeserializeOwned>(value: Value) -> ProtocolResult<T> {
    serde_json::from_value(value)
        .map_err(|_| protocol_error(CodexSdkProtocolErrorCode::MalformedFrame))
}

/// `serde_json::Value` keeps the last occurrence of a duplicate object key.
/// This wrapper rejects duplicates recursively before any frame routing or DTO
/// projection occurs.
struct StrictJsonValue(Value);

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(StrictJsonValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value.into())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        StrictJsonValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictJsonValue>()? {
            values.push(value.0);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = map.next_entry::<String, StrictJsonValue>()? {
            if values.insert(key, value.0).is_some() {
                return Err(A::Error::custom("duplicate JSON object key"));
            }
        }
        Ok(StrictJsonValue(Value::Object(values)))
    }
}

fn validate_frame_bytes(bytes: &[u8]) -> ProtocolResult<()> {
    if bytes.is_empty()
        || bytes.len() > MAX_CODEX_SDK_FRAME_BYTES
        || bytes
            .iter()
            .any(|byte| matches!(*byte, b'\0' | b'\r' | b'\n'))
    {
        return Err(protocol_error(if bytes.len() > MAX_CODEX_SDK_FRAME_BYTES {
            CodexSdkProtocolErrorCode::OversizedFrame
        } else {
            CodexSdkProtocolErrorCode::MalformedFrame
        }));
    }
    Ok(())
}

fn validate_id(value: &str, maximum: usize) -> ProtocolResult<()> {
    if value.is_empty()
        || value.len() > maximum
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField))
    } else {
        Ok(())
    }
}

fn validate_request_id(value: &str) -> ProtocolResult<()> {
    if value.is_empty()
        || value.len() > MAX_REQUEST_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField))
    } else {
        Ok(())
    }
}

fn valid_error_code(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginCompletedEvent {
    #[serde(rename = "kind")]
    _kind: LoginCompletedKind,
    #[serde(rename = "loginId")]
    login_id: String,
    success: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LoginCompletedKind {
    LoginCompleted,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnStartedEvent {
    #[serde(rename = "kind")]
    _kind: TurnStartedKind,
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TurnStartedKind {
    TurnStarted,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssistantDeltaEvent {
    #[serde(rename = "kind")]
    _kind: AssistantDeltaKind,
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssistantDeltaKind {
    AssistantDelta,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnCompletedEvent {
    #[serde(rename = "kind")]
    _kind: TurnCompletedKind,
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    status: CodexSdkTurnStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TurnCompletedKind {
    TurnCompleted,
}

fn parse_event(value: Value) -> ProtocolResult<CodexSdkStreamEvent> {
    let kind = value
        .as_object()
        .and_then(|object| object.get("kind"))
        .and_then(Value::as_str)
        .ok_or_else(|| protocol_error(CodexSdkProtocolErrorCode::MalformedFrame))?;
    let event = match kind {
        "login_completed" => {
            let event: LoginCompletedEvent = decode(value)?;
            validate_id(&event.login_id, MAX_LOGIN_ID_BYTES)?;
            CodexSdkStreamEvent::LoginCompleted {
                login_id: event.login_id,
                success: event.success,
            }
        }
        "turn_started" => {
            let event: TurnStartedEvent = decode(value)?;
            validate_id(&event.thread_id, MAX_ID_BYTES)?;
            validate_id(&event.turn_id, MAX_ID_BYTES)?;
            CodexSdkStreamEvent::TurnStarted {
                thread_id: event.thread_id,
                turn_id: event.turn_id,
            }
        }
        "assistant_delta" => {
            let event: AssistantDeltaEvent = decode(value)?;
            validate_id(&event.thread_id, MAX_ID_BYTES)?;
            validate_id(&event.turn_id, MAX_ID_BYTES)?;
            if event.text.is_empty() || event.text.len() > 64 * 1024 {
                return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField));
            }
            CodexSdkStreamEvent::AssistantDelta {
                thread_id: event.thread_id,
                turn_id: event.turn_id,
                text: event.text,
            }
        }
        "turn_completed" => {
            let event: TurnCompletedEvent = decode(value)?;
            validate_id(&event.thread_id, MAX_ID_BYTES)?;
            validate_id(&event.turn_id, MAX_ID_BYTES)?;
            CodexSdkStreamEvent::TurnCompleted {
                thread_id: event.thread_id,
                turn_id: event.turn_id,
                status: event.status,
            }
        }
        // This includes reasoning, tools, raw SDK notifications, and future
        // events. Nothing outside the audited projection is forwarded.
        _ => return Err(protocol_error(CodexSdkProtocolErrorCode::MalformedFrame)),
    };
    Ok(event)
}

pub fn empty_params() -> Value {
    Value::Object(Map::new())
}

pub fn login_start_params(kind: CodexSdkLoginKind) -> Value {
    serde_json::json!({ "kind": kind })
}

pub fn login_id_params(login_id: &str) -> ProtocolResult<Value> {
    validate_id(login_id, MAX_LOGIN_ID_BYTES)?;
    Ok(serde_json::json!({ "loginId": login_id }))
}

pub fn thread_start_params(cwd: &str, model: &str) -> ProtocolResult<Value> {
    validate_single_line(cwd, MAX_CWD_BYTES)?;
    validate_single_line(model, MAX_MODEL_BYTES)?;
    Ok(serde_json::json!({ "cwd": cwd, "model": model }))
}

pub fn thread_resume_params(cwd: &str, model: &str, thread_id: &str) -> ProtocolResult<Value> {
    validate_id(thread_id, MAX_ID_BYTES)?;
    let mut params = thread_start_params(cwd, model)?;
    params
        .as_object_mut()
        .expect("thread params are constructed as an object")
        .insert("threadId".into(), Value::String(thread_id.into()));
    Ok(params)
}

pub fn turn_start_params(
    cwd: &str,
    model: &str,
    operation_id: &str,
    prompt: &str,
    thread_id: &str,
) -> ProtocolResult<Value> {
    validate_single_line(cwd, MAX_CWD_BYTES)?;
    validate_single_line(model, MAX_MODEL_BYTES)?;
    validate_request_id(operation_id)?;
    validate_text(prompt, MAX_PROMPT_BYTES)?;
    validate_id(thread_id, MAX_ID_BYTES)?;
    Ok(serde_json::json!({
        "cwd": cwd,
        "model": model,
        "operationId": operation_id,
        "prompt": prompt,
        "threadId": thread_id,
    }))
}

pub fn turn_cancel_params(operation_id: &str) -> ProtocolResult<Value> {
    validate_request_id(operation_id)?;
    Ok(serde_json::json!({ "operationId": operation_id }))
}

pub fn approval_params(
    approval_id: &str,
    decision: CodexSdkApprovalDecision,
) -> ProtocolResult<Value> {
    validate_request_id(approval_id)?;
    Ok(serde_json::json!({ "approvalId": approval_id, "decision": decision }))
}

fn validate_text(value: &str, maximum: usize) -> ProtocolResult<()> {
    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField))
    } else {
        Ok(())
    }
}

fn validate_single_line(value: &str, maximum: usize) -> ProtocolResult<()> {
    if value.is_empty()
        || value.len() > maximum
        || value
            .chars()
            .any(|character| matches!(character, '\0' | '\r' | '\n'))
    {
        Err(protocol_error(CodexSdkProtocolErrorCode::InvalidField))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkCapabilities {
    pub account: bool,
    pub browser_login: bool,
    pub device_code_login: bool,
    pub experimental_api: bool,
    pub host_approval_blocker: String,
    pub host_approvals: bool,
    pub logout: bool,
    pub models: bool,
    pub sdk_version: String,
    pub streamed_turns: bool,
    pub thread_create: bool,
    pub thread_resume: bool,
    pub turn_cancellation: bool,
    pub usage: bool,
}

impl CodexSdkCapabilities {
    pub fn validate(&self) -> ProtocolResult<()> {
        let stable_surface = self.account
            && self.browser_login
            && self.device_code_login
            && !self.experimental_api
            && !self.host_approvals
            && self.host_approval_blocker == CODEX_SDK_HOST_APPROVAL_BLOCKER
            && self.logout
            && self.models
            && self.sdk_version == super::CODEX_SDK_RUNTIME_VERSION
            && self.streamed_turns
            && self.thread_create
            && self.thread_resume
            && self.turn_cancellation
            && !self.usage;
        if stable_surface {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::ProtocolMismatch))
        }
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkLoginPrompt {
    pub authorization_url: String,
    pub kind: CodexSdkLoginKindDto,
    pub login_id: String,
    pub user_code: Option<String>,
}

impl fmt::Debug for CodexSdkLoginPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexSdkLoginPrompt")
            .field("kind", &self.kind)
            .field("authorization_url", &"[REDACTED URL]")
            .field("login_id", &"[REDACTED]")
            .field("user_code", &self.user_code.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSdkLoginKindDto {
    Browser,
    DeviceCode,
}

impl CodexSdkLoginPrompt {
    pub fn validate(&self) -> ProtocolResult<()> {
        validate_id(&self.login_id, MAX_LOGIN_ID_BYTES)?;
        let url = Url::parse(&self.authorization_url)
            .map_err(|_| protocol_error(CodexSdkProtocolErrorCode::InvalidResult))?;
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult));
        }
        let host = url.host_str().unwrap_or_default();
        let safe = match self.kind {
            CodexSdkLoginKindDto::Browser => matches!(host, "chatgpt.com" | "auth.openai.com"),
            CodexSdkLoginKindDto::DeviceCode => {
                host == "auth.openai.com"
                    && url.path() == "/codex/device"
                    && self
                        .user_code
                        .as_deref()
                        .is_some_and(|code| !code.is_empty() && code.len() <= 64)
            }
        };
        if !safe || self.authorization_url.len() > 4096 {
            return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkLoginWait {
    pub accepted: bool,
    pub login_id: String,
}

impl CodexSdkLoginWait {
    pub fn validate(&self) -> ProtocolResult<()> {
        validate_id(&self.login_id, MAX_LOGIN_ID_BYTES)?;
        if self.accepted {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkLoginCancellation {
    pub cancelled: bool,
    pub login_id: String,
}

impl CodexSdkLoginCancellation {
    pub fn validate(&self) -> ProtocolResult<()> {
        validate_id(&self.login_id, MAX_LOGIN_ID_BYTES)?;
        if self.cancelled {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkAccount {
    pub authenticated: bool,
    #[serde(default)]
    pub auth_mode: Option<String>,
    #[serde(default)]
    pub display_label: Option<String>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub requires_openai_auth: Option<bool>,
}

impl CodexSdkAccount {
    pub fn validate(&self) -> ProtocolResult<()> {
        let valid = if self.authenticated {
            self.auth_mode.as_deref() == Some("chatgpt")
                && self.display_label.as_ref().map_or(true, |value| {
                    validate_single_line(value, 320).is_ok() && !contains_secret_marker(value)
                })
                && self.plan_type.as_ref().map_or(true, |value| {
                    validate_single_line(value, 128).is_ok() && !contains_secret_marker(value)
                })
                && self.requires_openai_auth.is_some()
        } else {
            self.auth_mode.is_none()
                && self.display_label.is_none()
                && self.plan_type.is_none()
                && self.requires_openai_auth.is_none()
        };
        if valid {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkModel {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexSdkModels {
    pub models: Vec<CodexSdkModel>,
}

impl CodexSdkModels {
    pub fn validate(&self) -> ProtocolResult<()> {
        if self.models.len() > 256 {
            return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult));
        }
        for (index, model) in self.models.iter().enumerate() {
            validate_single_line(&model.id, MAX_MODEL_BYTES)?;
            validate_single_line(&model.label, MAX_MODEL_BYTES)?;
            if contains_secret_marker(&model.id)
                || contains_secret_marker(&model.label)
                || self.models[..index]
                    .iter()
                    .any(|candidate| candidate.id == model.id)
            {
                return Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkThread {
    pub thread_id: String,
}

impl CodexSdkThread {
    pub fn validate(&self) -> ProtocolResult<()> {
        validate_id(&self.thread_id, MAX_ID_BYTES)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkTurn {
    pub operation_id: String,
    pub thread_id: String,
    pub turn_id: String,
}

impl CodexSdkTurn {
    pub fn validate(&self) -> ProtocolResult<()> {
        validate_request_id(&self.operation_id)?;
        validate_id(&self.thread_id, MAX_ID_BYTES)?;
        validate_id(&self.turn_id, MAX_ID_BYTES)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkTurnCancellation {
    pub cancelled: bool,
    pub operation_id: String,
}

impl CodexSdkTurnCancellation {
    pub fn validate(&self) -> ProtocolResult<()> {
        validate_request_id(&self.operation_id)?;
        if self.cancelled {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CodexSdkLogout {
    pub logged_out: bool,
    pub profile_state: String,
}

impl CodexSdkLogout {
    pub fn validate(&self) -> ProtocolResult<()> {
        if self.logged_out && self.profile_state == "logged_out" {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexSdkShutdown {
    pub closed: bool,
}

impl CodexSdkShutdown {
    pub fn validate(&self) -> ProtocolResult<()> {
        if self.closed {
            Ok(())
        } else {
            Err(protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
        }
    }
}

pub(super) fn parse_result<T: DeserializeOwned>(response: &CodexSdkResponse) -> ProtocolResult<T> {
    serde_json::from_value(response.result.clone())
        .map_err(|_| protocol_error(CodexSdkProtocolErrorCode::InvalidResult))
}

pub struct CodexSdkEventMapper {
    next_sequence: u32,
    normalizer: NativeEventNormalizer,
}

impl CodexSdkEventMapper {
    pub fn new(limits: NativeEventLimits) -> Result<Self, NativeRuntimeError> {
        Ok(Self {
            next_sequence: 0,
            normalizer: NativeEventNormalizer::new(limits)?,
        })
    }

    pub fn map(
        &mut self,
        event: CodexSdkStreamEvent,
    ) -> Result<Option<NativeEvent>, NativeRuntimeError> {
        let mut native = match event {
            CodexSdkStreamEvent::LoginCompleted { .. } => return Ok(None),
            CodexSdkStreamEvent::TurnStarted { thread_id, turn_id } => {
                let mut event = self.event(NativeEventKind::TurnStarted)?;
                event.session_id = Some(thread_id);
                event.turn_id = Some(turn_id);
                event
            }
            CodexSdkStreamEvent::AssistantDelta {
                thread_id,
                turn_id,
                text,
            } => {
                let mut event = self.event(NativeEventKind::AssistantDelta)?;
                event.content_class = Some(NativeContentClass::Assistant);
                event.session_id = Some(thread_id);
                event.turn_id = Some(turn_id);
                event.text = Some(text);
                event
            }
            CodexSdkStreamEvent::TurnCompleted {
                thread_id,
                turn_id,
                status,
            } => {
                let mut event = match status {
                    CodexSdkTurnStatus::Completed => self.event(NativeEventKind::TurnCompleted)?,
                    CodexSdkTurnStatus::Interrupted => {
                        self.event(NativeEventKind::TurnCancelled)?
                    }
                    CodexSdkTurnStatus::Failed => {
                        let mut event = self.event(NativeEventKind::TurnFailed)?;
                        event.error = Some("Codex turn failed".into());
                        event
                    }
                };
                event.session_id = Some(thread_id);
                event.turn_id = Some(turn_id);
                event
            }
        };
        native.metadata.clear();
        self.normalizer.normalize(native).map(Some)
    }

    pub fn session_started(
        &mut self,
        thread_id: String,
    ) -> Result<NativeEvent, NativeRuntimeError> {
        let mut event = self.event(NativeEventKind::SessionStarted)?;
        event.session_id = Some(thread_id);
        self.normalizer.normalize(event)
    }

    fn event(&mut self, kind: NativeEventKind) -> Result<NativeEvent, NativeRuntimeError> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::EventLimitExceeded,
                "Codex SDK event sequence exhausted",
                false,
            )
        })?;
        Ok(NativeEvent::new(sequence, kind))
    }
}
