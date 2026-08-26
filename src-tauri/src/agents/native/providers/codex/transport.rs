use super::{CodexMethod, CodexNotificationMethod, CodexServerRequestMethod};
use crate::agents::native::redact_text;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const MAX_CODEX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_CODEX_PENDING_REQUESTS: usize = 64;
pub const MAX_CODEX_INCOMING_QUEUE: usize = 128;
pub const MAX_CODEX_STDERR_BYTES: usize = 64 * 1024;
pub const MAX_CODEX_STDERR_LINE_BYTES: usize = 8 * 1024;
pub const DEFAULT_CODEX_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Incremental JSONL splitter. It bounds an unterminated line before parsing,
/// which prevents a fake or compromised process from growing memory forever.
#[derive(Debug, Default)]
pub struct CodexJsonlDecoder {
    buffered: Vec<u8>,
}

impl CodexJsonlDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, CodexTransportError> {
        self.buffered.extend_from_slice(bytes);
        if self.buffered.len() > MAX_CODEX_FRAME_BYTES
            && !self.buffered[..MAX_CODEX_FRAME_BYTES].contains(&b'\n')
        {
            self.buffered.clear();
            return Err(CodexTransportError::OversizedFrame);
        }
        let mut frames = Vec::new();
        while let Some(newline) = self.buffered.iter().position(|byte| *byte == b'\n') {
            let mut frame = self.buffered.drain(..=newline).collect::<Vec<_>>();
            frame.pop();
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            if frame.is_empty() {
                continue;
            }
            if frame.len() > MAX_CODEX_FRAME_BYTES {
                return Err(CodexTransportError::OversizedFrame);
            }
            frames.push(frame);
        }
        if self.buffered.len() > MAX_CODEX_FRAME_BYTES {
            self.buffered.clear();
            return Err(CodexTransportError::OversizedFrame);
        }
        Ok(frames)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CodexTransportError {
    #[error("app-server frame is malformed")]
    MalformedFrame,
    #[error("app-server frame exceeds the configured limit")]
    OversizedFrame,
    #[error("app-server request queue is full")]
    RequestQueueFull,
    #[error("app-server incoming queue is full")]
    IncomingQueueFull,
    #[error("app-server returned an unknown response id")]
    UnknownResponseId,
    #[error("app-server response id is invalid")]
    InvalidResponseId,
    #[error("app-server request method is not supported")]
    UnsupportedServerRequest,
    #[error("app-server request cannot be sent before initialization")]
    NotInitialized,
    #[error("app-server initialize request is invalid for this connection")]
    InvalidInitializationState,
    #[error("app-server initialization response does not match the pinned protocol")]
    ProtocolMismatch,
    #[error("app-server request timed out")]
    TimedOut,
    #[error("app-server exited while requests were pending")]
    ProcessExited,
    #[error("app-server JSON could not be encoded")]
    EncodeFailed,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    method: CodexMethod,
    deadline: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexResponse {
    pub id: u64,
    pub method: CodexMethod,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexNotification {
    pub method: CodexNotificationMethod,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexServerRequest {
    pub id: Value,
    pub method: CodexServerRequestMethod,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexIncoming {
    Response(CodexResponse),
    Notification(CodexNotification),
    ServerRequest(CodexServerRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTimedOutRequest {
    pub id: u64,
    pub method: CodexMethod,
}

/// Bounded JSONL request router. It owns IDs, deadlines, and inbound queueing,
/// while the process adapter owns only bytes and lifecycle.
pub struct CodexJsonlTransport {
    next_id: u64,
    pending: BTreeMap<u64, PendingRequest>,
    incoming: VecDeque<CodexIncoming>,
    initialize_sent: bool,
    initialized: bool,
}

impl Default for CodexJsonlTransport {
    fn default() -> Self {
        Self {
            next_id: 1,
            pending: BTreeMap::new(),
            incoming: VecDeque::new(),
            initialize_sent: false,
            initialized: false,
        }
    }
}

impl CodexJsonlTransport {
    pub fn encode_request(
        &mut self,
        method: CodexMethod,
        params: Value,
        now: Instant,
        timeout: Duration,
    ) -> Result<(u64, Vec<u8>), CodexTransportError> {
        if timeout.is_zero() {
            return Err(CodexTransportError::TimedOut);
        }
        match method {
            CodexMethod::Initialize if self.initialize_sent || self.initialized => {
                return Err(CodexTransportError::InvalidInitializationState)
            }
            CodexMethod::Initialize => self.initialize_sent = true,
            _ if !self.initialized => return Err(CodexTransportError::NotInitialized),
            _ => {}
        }
        if self.pending.len() >= MAX_CODEX_PENDING_REQUESTS {
            return Err(CodexTransportError::RequestQueueFull);
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(1);
        if self.pending.contains_key(&id) {
            return Err(CodexTransportError::RequestQueueFull);
        }
        let mut request = Map::new();
        request.insert("method".into(), Value::String(method.as_str().into()));
        request.insert("id".into(), Value::from(id));
        if !params.is_null() {
            request.insert("params".into(), params);
        }
        let frame = encode_frame(&Value::Object(request))?;
        self.pending.insert(
            id,
            PendingRequest {
                method,
                deadline: now + timeout,
            },
        );
        Ok((id, frame))
    }

    /// Validates the version-specific initialize response and emits the one
    /// required `initialized` notification. The official response has no
    /// independent protocol-version field, so the runtime artifact version and
    /// digest must be checked before process launch.
    pub fn accept_initialize_response(
        &mut self,
        response: &CodexResponse,
        expected_home: &Path,
    ) -> Result<Vec<u8>, CodexTransportError> {
        if self.initialized
            || response.method != CodexMethod::Initialize
            || response.error.is_some()
        {
            return Err(CodexTransportError::InvalidInitializationState);
        }
        let result = response
            .result
            .as_ref()
            .and_then(Value::as_object)
            .ok_or(CodexTransportError::ProtocolMismatch)?;
        require_nonempty_string(result, "userAgent")?;
        require_nonempty_string(result, "platformFamily")?;
        require_nonempty_string(result, "platformOs")?;
        let returned_home = require_nonempty_string(result, "codexHome")?;
        if Path::new(returned_home) != expected_home {
            return Err(CodexTransportError::ProtocolMismatch);
        }
        self.initialized = true;
        encode_frame(&json!({ "method": "initialized" }))
    }

    pub fn ingest(&mut self, line: &[u8]) -> Result<bool, CodexTransportError> {
        if line.len() > MAX_CODEX_FRAME_BYTES {
            return Err(CodexTransportError::OversizedFrame);
        }
        let value: Value =
            serde_json::from_slice(line).map_err(|_| CodexTransportError::MalformedFrame)?;
        let object = value
            .as_object()
            .ok_or(CodexTransportError::MalformedFrame)?;
        if let Some(method) = object.get("method").and_then(Value::as_str) {
            let params = object.get("params").cloned().unwrap_or(Value::Null);
            if let Some(id) = object.get("id") {
                let method = CodexServerRequestMethod::parse(method)
                    .ok_or(CodexTransportError::UnsupportedServerRequest)?;
                if !(id.is_u64() || id.is_i64() || id.is_string()) {
                    return Err(CodexTransportError::InvalidResponseId);
                }
                self.push(CodexIncoming::ServerRequest(CodexServerRequest {
                    id: id.clone(),
                    method,
                    params,
                }))?;
                return Ok(true);
            }
            let Some(method) = CodexNotificationMethod::parse(method) else {
                // Forward compatibility: unknown notifications are ignored,
                // bounded at the frame boundary, and never enter Alfred state.
                return Ok(false);
            };
            self.push(CodexIncoming::Notification(CodexNotification {
                method,
                params,
            }))?;
            return Ok(true);
        }

        let id = object
            .get("id")
            .and_then(Value::as_u64)
            .ok_or(CodexTransportError::InvalidResponseId)?;
        let has_result = object.contains_key("result");
        let has_error = object.contains_key("error");
        if has_result == has_error {
            return Err(CodexTransportError::MalformedFrame);
        }
        let pending = self
            .pending
            .remove(&id)
            .ok_or(CodexTransportError::UnknownResponseId)?;
        self.push(CodexIncoming::Response(CodexResponse {
            id,
            method: pending.method,
            result: object.get("result").cloned(),
            error: object.get("error").map(sanitize_error_value),
        }))?;
        Ok(true)
    }

    pub fn pop(&mut self) -> Option<CodexIncoming> {
        self.incoming.pop_front()
    }

    pub fn expire(&mut self, now: Instant) -> Vec<CodexTimedOutRequest> {
        let expired = self
            .pending
            .iter()
            .filter(|(_, request)| request.deadline <= now)
            .map(|(id, request)| CodexTimedOutRequest {
                id: *id,
                method: request.method,
            })
            .collect::<Vec<_>>();
        for request in &expired {
            self.pending.remove(&request.id);
        }
        expired
    }

    pub fn process_exited(&mut self) -> Vec<CodexTimedOutRequest> {
        let interrupted = self
            .pending
            .iter()
            .map(|(id, request)| CodexTimedOutRequest {
                id: *id,
                method: request.method,
            })
            .collect();
        self.pending.clear();
        interrupted
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn push(&mut self, message: CodexIncoming) -> Result<(), CodexTransportError> {
        if self.incoming.len() >= MAX_CODEX_INCOMING_QUEUE {
            return Err(CodexTransportError::IncomingQueueFull);
        }
        self.incoming.push_back(message);
        Ok(())
    }
}

fn require_nonempty_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, CodexTransportError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(CodexTransportError::ProtocolMismatch)
}

fn sanitize_error_value(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(redact_text(value)),
        Value::Array(values) => Value::Array(values.iter().map(sanitize_error_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if crate::agents::native::is_secret_key(key) {
                        Value::String("[REDACTED]".into())
                    } else {
                        sanitize_error_value(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub fn encode_server_response(id: Value, result: Value) -> Result<Vec<u8>, CodexTransportError> {
    if !(id.is_u64() || id.is_i64() || id.is_string()) {
        return Err(CodexTransportError::InvalidResponseId);
    }
    encode_frame(&json!({ "id": id, "result": result }))
}

fn encode_frame(value: &Value) -> Result<Vec<u8>, CodexTransportError> {
    let mut encoded = serde_json::to_vec(value).map_err(|_| CodexTransportError::EncodeFailed)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_CODEX_FRAME_BYTES {
        return Err(CodexTransportError::OversizedFrame);
    }
    Ok(encoded)
}

/// Bounded stderr tail. Every line is redacted before retention and long
/// process output evicts old content instead of growing without limit.
#[derive(Debug, Default)]
pub struct CodexStderrTail {
    lines: VecDeque<String>,
    bytes: usize,
}

impl CodexStderrTail {
    pub fn push(&mut self, line: &str) {
        let bounded = if line.len() > MAX_CODEX_STDERR_LINE_BYTES {
            let mut end = MAX_CODEX_STDERR_LINE_BYTES;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &line[..end])
        } else {
            line.to_owned()
        };
        let redacted = redact_text(&bounded);
        self.bytes += redacted.len();
        self.lines.push_back(redacted);
        while self.bytes > MAX_CODEX_STDERR_BYTES {
            let Some(removed) = self.lines.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.len());
        }
    }

    pub fn render(&self) -> String {
        self.lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn initialized_transport(now: Instant) -> CodexJsonlTransport {
        let mut transport = CodexJsonlTransport::default();
        let (id, _) = transport
            .encode_request(
                CodexMethod::Initialize,
                json!({}),
                now,
                DEFAULT_CODEX_REQUEST_TIMEOUT,
            )
            .unwrap();
        transport
            .ingest(
                json!({
                    "id": id,
                    "result": {
                        "userAgent": "codex/0.149.1",
                        "codexHome": "/alfred/codex-home",
                        "platformFamily": "unix",
                        "platformOs": "macos"
                    }
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();
        let CodexIncoming::Response(response) = transport.pop().unwrap() else {
            panic!("initialize response")
        };
        transport
            .accept_initialize_response(&response, &PathBuf::from("/alfred/codex-home"))
            .unwrap();
        transport
    }

    #[test]
    fn malformed_oversized_and_unknown_id_frames_are_rejected() {
        let mut transport = CodexJsonlTransport::default();
        assert_eq!(
            transport.ingest(b"not-json").unwrap_err(),
            CodexTransportError::MalformedFrame
        );
        assert_eq!(
            transport
                .ingest(&vec![b'x'; MAX_CODEX_FRAME_BYTES + 1])
                .unwrap_err(),
            CodexTransportError::OversizedFrame
        );
        assert_eq!(
            transport.ingest(br#"{"id":991,"result":{}}"#).unwrap_err(),
            CodexTransportError::UnknownResponseId
        );
        let now = Instant::now();
        let mut initialized = initialized_transport(now);
        let (id, _) = initialized
            .encode_request(
                CodexMethod::AccountRead,
                json!({}),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            initialized
                .ingest(
                    json!({"id":id,"result":{},"error":{"message":"both"}})
                        .to_string()
                        .as_bytes()
                )
                .unwrap_err(),
            CodexTransportError::MalformedFrame
        );
    }

    #[test]
    fn jsonl_decoder_handles_split_and_multiple_frames_with_an_unterminated_bound() {
        let mut decoder = CodexJsonlDecoder::default();
        assert!(decoder.push(br#"{"method":"turn/"#).unwrap().is_empty());
        let frames = decoder
            .push(b"started\",\"params\":{}}\n{\"method\":\"configWarning\",\"params\":{}}\r\n")
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            serde_json::from_slice::<Value>(&frames[0]).unwrap()["method"],
            "turn/started"
        );
        assert_eq!(
            decoder
                .push(&vec![b'x'; MAX_CODEX_FRAME_BYTES + 1])
                .unwrap_err(),
            CodexTransportError::OversizedFrame
        );
    }

    #[test]
    fn ids_timeouts_exit_and_cancel_requests_are_bounded() {
        let now = Instant::now();
        let mut transport = initialized_transport(now);
        let (first, _) = transport
            .encode_request(
                CodexMethod::AccountRead,
                json!({}),
                now,
                Duration::from_millis(1),
            )
            .unwrap();
        let (second, interrupt) = transport
            .encode_request(
                CodexMethod::TurnInterrupt,
                json!({"threadId":"thr-1","turnId":"turn-1"}),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(second > first);
        assert!(String::from_utf8(interrupt)
            .unwrap()
            .contains("turn/interrupt"));
        assert_eq!(
            transport.expire(now + Duration::from_millis(2))[0].id,
            first
        );
        assert_eq!(transport.process_exited()[0].id, second);
        assert_eq!(transport.pending_len(), 0);
    }

    #[test]
    fn handshake_rejects_protocol_shape_and_runtime_home_mismatches() {
        let now = Instant::now();
        let mut transport = CodexJsonlTransport::default();
        let (id, _) = transport
            .encode_request(
                CodexMethod::Initialize,
                json!({}),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        transport
            .ingest(
                json!({"id":id,"result":{"userAgent":"codex","codexHome":"/user/.codex"}})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        let CodexIncoming::Response(response) = transport.pop().unwrap() else {
            panic!("response")
        };
        assert_eq!(
            transport
                .accept_initialize_response(&response, Path::new("/alfred/codex-home"))
                .unwrap_err(),
            CodexTransportError::ProtocolMismatch
        );
    }

    #[test]
    fn unknown_notifications_are_dropped_and_server_requests_are_allowlisted() {
        let now = Instant::now();
        let mut transport = initialized_transport(now);
        assert!(!transport
            .ingest(br#"{"method":"rawResponse/completed","params":{"reasoning":"secret"}}"#)
            .unwrap());
        assert!(transport.pop().is_none());
        assert_eq!(
            transport
                .ingest(br#"{"id":4,"method":"attestation/generate","params":{}}"#)
                .unwrap_err(),
            CodexTransportError::UnsupportedServerRequest
        );
        transport
            .ingest(
                br#"{"id":5,"method":"item/fileChange/requestApproval","params":{"itemId":"i"}}"#,
            )
            .unwrap();
        assert!(matches!(
            transport.pop(),
            Some(CodexIncoming::ServerRequest(_))
        ));
    }

    #[test]
    fn queue_backpressure_is_enforced() {
        let now = Instant::now();
        let mut transport = initialized_transport(now);
        for _ in 0..MAX_CODEX_PENDING_REQUESTS {
            transport
                .encode_request(
                    CodexMethod::AccountRead,
                    json!({}),
                    now,
                    Duration::from_secs(1),
                )
                .unwrap();
        }
        assert_eq!(
            transport
                .encode_request(
                    CodexMethod::AccountRead,
                    json!({}),
                    now,
                    Duration::from_secs(1),
                )
                .unwrap_err(),
            CodexTransportError::RequestQueueFull
        );
    }

    #[test]
    fn stderr_is_redacted_and_bounded() {
        let mut tail = CodexStderrTail::default();
        tail.push("Authorization: Bearer secret-token");
        tail.push("api sk-live-secret");
        let output = tail.render();
        assert!(!output.contains("secret-token"));
        assert!(!output.contains("sk-live-secret"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn provider_error_payloads_are_redacted_before_queueing() {
        let now = Instant::now();
        let mut transport = initialized_transport(now);
        let (id, _) = transport
            .encode_request(
                CodexMethod::AccountRead,
                json!({}),
                now,
                Duration::from_secs(1),
            )
            .unwrap();
        transport
            .ingest(
                json!({
                    "id":id,
                    "error":{
                        "message":"Authorization: Bearer provider-secret",
                        "accessToken":"token-secret"
                    }
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();
        let rendered = format!("{:?}", transport.pop().unwrap());
        assert!(!rendered.contains("provider-secret"));
        assert!(!rendered.contains("token-secret"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
