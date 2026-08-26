use crate::agents::native::{
    contains_secret_marker, redact_text, NativeContentClass, NativeErrorCode, NativeEvent,
    NativeEventKind, NativeRuntimeError,
};
use serde_json::{Map, Value};

const MAX_WIRE_EVENT_BYTES: usize = 256 * 1024;
const MAX_WIRE_ID_BYTES: usize = 128;
const MAX_ASSISTANT_DELTA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenCodeToolPermission {
    Pending {
        permission_id: String,
        session_id: String,
        permission_type: String,
        title: String,
    },
    Replied {
        permission_id: String,
        session_id: String,
        approved: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenCodeProtocolEvent {
    Connected,
    SessionStarted { session_id: String },
    AssistantDelta(NativeEvent),
    ToolPermission(OpenCodeToolPermission),
    SessionIdle { session_id: String },
    SessionError { session_id: Option<String> },
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodeServerFailure {
    Unauthorized,
    Forbidden,
    RateLimited,
    Unavailable,
    Protocol,
}

pub fn map_http_failure(failure: OpenCodeServerFailure) -> NativeRuntimeError {
    match failure {
        OpenCodeServerFailure::Unauthorized | OpenCodeServerFailure::Forbidden => {
            NativeRuntimeError::new(
                NativeErrorCode::AccountUnavailable,
                "OpenCode upstream account is unavailable; reconnect the explicit billing account",
                false,
            )
        }
        OpenCodeServerFailure::RateLimited => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "OpenCode upstream provider rate limit reached",
            true,
        ),
        OpenCodeServerFailure::Unavailable => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "OpenCode isolated server is unavailable",
            true,
        ),
        OpenCodeServerFailure::Protocol => NativeRuntimeError::new(
            NativeErrorCode::InvalidEvent,
            "OpenCode server protocol response is invalid",
            false,
        ),
    }
}

/// Decode only the documented OpenCode event variants Alfred needs.
///
/// Unknown variants are ignored rather than exposed as arbitrary server
/// passthrough. Tool metadata is deliberately not accepted as executable input:
/// OpenCode declares it as `{ [key: string]: unknown }` in 1.18.23.
pub fn decode_server_event(
    value: Value,
    expected_session_id: Option<&str>,
    sequence: u32,
) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
    let encoded_len = serde_json::to_vec(&value)
        .map_err(|_| invalid_event("OpenCode event could not be encoded"))?
        .len();
    if encoded_len > MAX_WIRE_EVENT_BYTES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "OpenCode wire event exceeded the bounded transport limit",
            false,
        ));
    }
    let object = value
        .as_object()
        .ok_or_else(|| invalid_event("OpenCode event must be an object"))?;
    let event_type = required_string(object, "type")?;
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_event("OpenCode event properties are missing"))?;

    match event_type {
        "server.connected" => Ok(OpenCodeProtocolEvent::Connected),
        "session.created" => {
            let info = properties
                .get("info")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid_event("OpenCode session info is missing"))?;
            let session_id = wire_id(required_string(info, "id")?, "session id")?;
            validate_expected_session(expected_session_id, &session_id)?;
            Ok(OpenCodeProtocolEvent::SessionStarted { session_id })
        }
        "message.part.updated" => decode_part(properties, expected_session_id, sequence),
        "permission.updated" => {
            let permission_id = wire_id(required_string(properties, "id")?, "permission id")?;
            let session_id = wire_id(required_string(properties, "sessionID")?, "session id")?;
            validate_expected_session(expected_session_id, &session_id)?;
            let permission_type =
                bounded_text(required_string(properties, "type")?, 64, "permission type")?;
            let title = redact_text(&bounded_text(
                required_string(properties, "title")?,
                512,
                "permission title",
            )?);
            Ok(OpenCodeProtocolEvent::ToolPermission(
                OpenCodeToolPermission::Pending {
                    permission_id,
                    session_id,
                    permission_type,
                    title,
                },
            ))
        }
        "permission.replied" => {
            let permission_id = wire_id(
                required_string(properties, "permissionID")?,
                "permission id",
            )?;
            let session_id = wire_id(required_string(properties, "sessionID")?, "session id")?;
            validate_expected_session(expected_session_id, &session_id)?;
            let response = required_string(properties, "response")?;
            let approved = match response {
                "once" | "always" => true,
                "reject" => false,
                _ => return Err(invalid_event("OpenCode permission response is invalid")),
            };
            Ok(OpenCodeProtocolEvent::ToolPermission(
                OpenCodeToolPermission::Replied {
                    permission_id,
                    session_id,
                    approved,
                },
            ))
        }
        "session.idle" => {
            let session_id = wire_id(required_string(properties, "sessionID")?, "session id")?;
            validate_expected_session(expected_session_id, &session_id)?;
            Ok(OpenCodeProtocolEvent::SessionIdle { session_id })
        }
        "session.error" => {
            let session_id = properties
                .get("sessionID")
                .and_then(Value::as_str)
                .map(|id| wire_id(id, "session id"))
                .transpose()?;
            if let Some(session_id) = session_id.as_deref() {
                validate_expected_session(expected_session_id, session_id)?;
            }
            Ok(OpenCodeProtocolEvent::SessionError { session_id })
        }
        _ => Ok(OpenCodeProtocolEvent::Ignored),
    }
}

fn decode_part(
    properties: &Map<String, Value>,
    expected_session_id: Option<&str>,
    sequence: u32,
) -> Result<OpenCodeProtocolEvent, NativeRuntimeError> {
    let part = properties
        .get("part")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_event("OpenCode message part is missing"))?;
    let session_id = wire_id(required_string(part, "sessionID")?, "session id")?;
    validate_expected_session(expected_session_id, &session_id)?;
    match required_string(part, "type")? {
        "text" => {
            let delta = properties
                .get("delta")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    invalid_event("OpenCode text update lacks a delta and cannot be deduplicated")
                })?;
            if delta.len() > MAX_ASSISTANT_DELTA_BYTES {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "OpenCode assistant delta exceeded the bounded text limit",
                    false,
                ));
            }
            let mut event = NativeEvent::new(sequence, NativeEventKind::AssistantDelta);
            event.content_class = Some(NativeContentClass::Assistant);
            event.session_id = Some(session_id);
            event.text = Some(redact_text(delta));
            Ok(OpenCodeProtocolEvent::AssistantDelta(event))
        }
        "reasoning" => Err(invalid_event(
            "OpenCode reasoning parts are prohibited at the Alfred event boundary",
        )),
        // Tool state is observable, but it is not executable input. The
        // official schema types its input/metadata as unknown.
        "tool" => Ok(OpenCodeProtocolEvent::Ignored),
        _ => Ok(OpenCodeProtocolEvent::Ignored),
    }
}

fn validate_expected_session(
    expected: Option<&str>,
    actual: &str,
) -> Result<(), NativeRuntimeError> {
    if expected.is_some_and(|expected| expected != actual) {
        Err(NativeRuntimeError::new(
            NativeErrorCode::SessionUnavailable,
            "OpenCode event belongs to a different session",
            false,
        ))
    } else {
        Ok(())
    }
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, NativeRuntimeError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_event(format!("OpenCode event field `{key}` is invalid")))
}

fn wire_id(value: &str, label: &str) -> Result<String, NativeRuntimeError> {
    if contains_secret_marker(value) {
        Err(invalid_event(format!(
            "OpenCode {label} contains prohibited material"
        )))
    } else {
        bounded_text(value, MAX_WIRE_ID_BYTES, label)
    }
}

fn bounded_text(value: &str, limit: usize, label: &str) -> Result<String, NativeRuntimeError> {
    if value.is_empty()
        || value.len() > limit
        || value
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        Err(invalid_event(format!("OpenCode {label} is invalid")))
    } else {
        Ok(value.into())
    }
}

fn invalid_event(message: impl Into<String>) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::InvalidEvent, message, false)
}
