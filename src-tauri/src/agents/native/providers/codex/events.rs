use super::{
    encode_server_response, CodexNotification, CodexNotificationMethod, CodexServerRequest,
    CodexServerRequestMethod, CodexTransportError,
};
use crate::agents::native::{
    AlfredApprovalDecision, AlfredApprovalRequest, AlfredToolKind, NativeContentClass,
    NativeErrorCode, NativeEvent, NativeEventKind, NativeRuntimeError, TOOL_CONTRACT_VERSION,
};
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

pub const MAX_CODEX_EVENT_FIELD_BYTES: usize = 128 * 1024;

/// Converts the stable, allowlisted app-server notifications into Plan 032
/// events. Reasoning item types and raw response events never produce output.
#[derive(Default)]
pub struct CodexEventMapper {
    next_sequence: u32,
    token_usage: Option<Map<String, Value>>,
}

impl CodexEventMapper {
    pub fn map(
        &mut self,
        notification: &CodexNotification,
    ) -> Result<Option<NativeEvent>, NativeRuntimeError> {
        let params = notification
            .params
            .as_object()
            .ok_or_else(invalid_provider_event)?;
        let event = match notification.method {
            CodexNotificationMethod::ThreadStarted => {
                let thread = required_object(params.get("thread"))?;
                let mut event = self.event(NativeEventKind::SessionStarted)?;
                event.session_id = Some(required_id(thread.get("id"))?);
                Some(event)
            }
            CodexNotificationMethod::TurnStarted => {
                let turn = required_object(params.get("turn"))?;
                let mut event = self.event(NativeEventKind::TurnStarted)?;
                event.turn_id = Some(required_id(turn.get("id"))?);
                event.session_id = optional_id(params.get("threadId"))?;
                Some(event)
            }
            CodexNotificationMethod::AgentMessageDelta => {
                let delta = bounded_string(params.get("delta"), MAX_CODEX_EVENT_FIELD_BYTES)?;
                let mut event = self.event(NativeEventKind::AssistantDelta)?;
                event.content_class = Some(NativeContentClass::Assistant);
                event.text = Some(delta);
                event.session_id = optional_id(params.get("threadId"))?;
                event.turn_id = optional_id(params.get("turnId"))?;
                Some(event)
            }
            CodexNotificationMethod::ItemStarted => self.map_item(params, false)?,
            CodexNotificationMethod::ItemCompleted => self.map_item(params, true)?,
            CodexNotificationMethod::CommandOutputDelta => {
                let mut event = self.event(NativeEventKind::ToolProgress)?;
                event.tool_call_id = Some(required_id(params.get("itemId"))?);
                event.tool_name = Some("shell".into());
                event.text = Some(bounded_string(
                    params.get("delta"),
                    MAX_CODEX_EVENT_FIELD_BYTES,
                )?);
                event.session_id = optional_id(params.get("threadId"))?;
                event.turn_id = optional_id(params.get("turnId"))?;
                Some(event)
            }
            CodexNotificationMethod::TurnDiffUpdated => {
                let mut event = self.event(NativeEventKind::ToolProgress)?;
                event.tool_call_id =
                    Some(format!("turn-diff-{}", required_id(params.get("turnId"))?));
                event.tool_name = Some("apply_patch".into());
                event.tool_output = Some(bounded_string(
                    params.get("diff"),
                    MAX_CODEX_EVENT_FIELD_BYTES,
                )?);
                event.session_id = optional_id(params.get("threadId"))?;
                event.turn_id = optional_id(params.get("turnId"))?;
                Some(event)
            }
            CodexNotificationMethod::TokenUsageUpdated => {
                self.token_usage = Some(numeric_usage(params.get("tokenUsage"))?);
                None
            }
            CodexNotificationMethod::TurnCompleted => self.map_turn_completed(params)?,
            CodexNotificationMethod::ConfigWarning => {
                let mut event = self.event(NativeEventKind::Warning)?;
                event.text = Some(bounded_string(
                    params.get("summary"),
                    MAX_CODEX_EVENT_FIELD_BYTES,
                )?);
                Some(event)
            }
            CodexNotificationMethod::AccountLoginCompleted
            | CodexNotificationMethod::AccountUpdated
            | CodexNotificationMethod::RateLimitsUpdated => None,
        };
        Ok(event)
    }

    fn map_item(
        &mut self,
        params: &Map<String, Value>,
        completed: bool,
    ) -> Result<Option<NativeEvent>, NativeRuntimeError> {
        let item = required_object(params.get("item"))?;
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(invalid_provider_event)?;
        // No reasoning content crosses the provider boundary.
        if matches!(item_type, "reasoning" | "plan" | "analysis") {
            return Ok(None);
        }
        let (tool_name, output) = match item_type {
            "commandExecution" => (
                "shell",
                item.get("aggregatedOutput")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            ),
            "fileChange" => (
                "apply_patch",
                item.get("changes").map(bounded_json).transpose()?,
            ),
            "mcpToolCall" => ("mcp", item.get("result").map(bounded_json).transpose()?),
            _ => return Ok(None),
        };
        let mut event = self.event(if completed {
            NativeEventKind::ToolCompleted
        } else {
            NativeEventKind::ToolStarted
        })?;
        event.tool_call_id = Some(required_id(item.get("id"))?);
        event.tool_name = Some(tool_name.into());
        if completed {
            event.tool_output = output;
        }
        event.session_id = optional_id(params.get("threadId"))?;
        event.turn_id = optional_id(params.get("turnId"))?;
        Ok(Some(event))
    }

    fn map_turn_completed(
        &mut self,
        params: &Map<String, Value>,
    ) -> Result<Option<NativeEvent>, NativeRuntimeError> {
        let turn = required_object(params.get("turn"))?;
        let status = turn
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(invalid_provider_event)?;
        let mut event = match status {
            "completed" => self.event(NativeEventKind::TurnCompleted)?,
            "interrupted" => self.event(NativeEventKind::TurnCancelled)?,
            "failed" => {
                let mut event = self.event(NativeEventKind::TurnFailed)?;
                let error = turn
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|error| error.get("message"))
                    .map(|value| bounded_string(Some(value), MAX_CODEX_EVENT_FIELD_BYTES))
                    .transpose()?
                    .unwrap_or_else(|| "Codex turn failed".into());
                event.error = Some(error);
                event
            }
            _ => return Err(invalid_provider_event()),
        };
        event.turn_id = Some(required_id(turn.get("id"))?);
        event.session_id = optional_id(params.get("threadId"))?;
        if let Some(usage) = self.token_usage.take() {
            event.metadata.insert("usage".into(), Value::Object(usage));
        }
        Ok(Some(event))
    }

    fn event(&mut self, kind: NativeEventKind) -> Result<NativeEvent, NativeRuntimeError> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::EventLimitExceeded,
                "Codex event sequence exhausted",
                false,
            )
        })?;
        Ok(NativeEvent::new(sequence, kind))
    }
}

/// Projects a server approval request into the frozen Alfred approval shape.
/// The approval-only seam remains distinct from tool execution.
pub fn project_approval(
    request: &CodexServerRequest,
    working_directory: &Path,
    allowed_roots: &[PathBuf],
) -> Result<AlfredApprovalRequest, NativeRuntimeError> {
    let params = request
        .params
        .as_object()
        .ok_or_else(invalid_provider_event)?;
    let item_id = required_id(params.get("itemId"))?;
    let (kind, tool_name) = match request.method {
        CodexServerRequestMethod::CommandApproval => {
            let cwd = params
                .get("cwd")
                .and_then(Value::as_str)
                .map(Path::new)
                .unwrap_or(working_directory);
            validate_workspace(cwd, allowed_roots)?;
            (AlfredToolKind::Shell, "shell")
        }
        CodexServerRequestMethod::FileChangeApproval => {
            if let Some(root) = params.get("grantRoot").and_then(Value::as_str) {
                validate_workspace(Path::new(root), allowed_roots)?;
            }
            (AlfredToolKind::ApplyPatch, "apply_patch")
        }
    };
    Ok(AlfredApprovalRequest {
        contract_version: TOOL_CONTRACT_VERSION,
        approval_id: server_id_label(&request.id)?,
        tool_request_id: item_id,
        tool_name: tool_name.into(),
        kind,
    })
}

pub fn encode_approval_decision(
    request: &CodexServerRequest,
    decision: AlfredApprovalDecision,
    cancelled: bool,
) -> Result<Vec<u8>, CodexTransportError> {
    let decision = if cancelled {
        "cancel"
    } else {
        match decision {
            AlfredApprovalDecision::Allow => "accept",
            AlfredApprovalDecision::Deny => "decline",
        }
    };
    encode_server_response(request.id.clone(), json!({ "decision": decision }))
}

fn validate_workspace(path: &Path, roots: &[PathBuf]) -> Result<(), NativeRuntimeError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || !roots.iter().any(|root| path.starts_with(root))
    {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::WorkspaceDenied,
            "Codex approval target is outside the allowed workspace",
            false,
        ));
    }
    Ok(())
}

fn numeric_usage(value: Option<&Value>) -> Result<Map<String, Value>, NativeRuntimeError> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(invalid_provider_event)?;
    let mut safe = Map::new();
    for (key, value) in object {
        if value.is_number() || value.is_null() {
            safe.insert(key.clone(), value.clone());
        }
    }
    Ok(safe)
}

fn required_object(value: Option<&Value>) -> Result<&Map<String, Value>, NativeRuntimeError> {
    value
        .and_then(Value::as_object)
        .ok_or_else(invalid_provider_event)
}

fn required_id(value: Option<&Value>) -> Result<String, NativeRuntimeError> {
    bounded_string(value, 128).and_then(|value| {
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            .then_some(value)
            .ok_or_else(invalid_provider_event)
    })
}

fn optional_id(value: Option<&Value>) -> Result<Option<String>, NativeRuntimeError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => required_id(Some(value)).map(Some),
    }
}

fn bounded_string(value: Option<&Value>, max: usize) -> Result<String, NativeRuntimeError> {
    value
        .and_then(Value::as_str)
        .filter(|value| value.len() <= max)
        .map(ToOwned::to_owned)
        .ok_or_else(invalid_provider_event)
}

fn bounded_json(value: &Value) -> Result<String, NativeRuntimeError> {
    let encoded = serde_json::to_string(value).map_err(|_| invalid_provider_event())?;
    if encoded.len() > MAX_CODEX_EVENT_FIELD_BYTES {
        return Err(NativeRuntimeError::new(
            NativeErrorCode::EventLimitExceeded,
            "Codex event field exceeded the provider limit",
            false,
        ));
    }
    Ok(encoded)
}

fn server_id_label(value: &Value) -> Result<String, NativeRuntimeError> {
    let label = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return Err(invalid_provider_event()),
    };
    if label.is_empty() || label.len() > 128 {
        return Err(invalid_provider_event());
    }
    Ok(label)
}

fn invalid_provider_event() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidEvent,
        "Codex app-server event is invalid",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::native::{NativeEventLimits, NativeEventNormalizer};
    use serde_json::json;

    fn notification(method: CodexNotificationMethod, params: Value) -> CodexNotification {
        CodexNotification { method, params }
    }

    #[test]
    fn prompt_tool_cancel_and_error_events_map_without_reasoning() {
        let mut mapper = CodexEventMapper::default();
        let delta = mapper
            .map(&notification(
                CodexNotificationMethod::AgentMessageDelta,
                json!({"threadId":"thr_1","turnId":"turn_1","delta":"hello"}),
            ))
            .unwrap()
            .unwrap();
        assert_eq!(delta.kind, NativeEventKind::AssistantDelta);
        assert_eq!(delta.content_class, Some(NativeContentClass::Assistant));

        let tool = mapper
            .map(&notification(
                CodexNotificationMethod::ItemCompleted,
                json!({
                    "threadId":"thr_1","turnId":"turn_1",
                    "item":{"type":"commandExecution","id":"item_1","aggregatedOutput":"ok"}
                }),
            ))
            .unwrap()
            .unwrap();
        assert_eq!(tool.kind, NativeEventKind::ToolCompleted);
        assert_eq!(tool.tool_output.as_deref(), Some("ok"));

        let reasoning = mapper
            .map(&notification(
                CodexNotificationMethod::ItemCompleted,
                json!({"item":{"type":"reasoning","id":"reason_1","text":"hidden"}}),
            ))
            .unwrap();
        assert!(reasoning.is_none());

        let cancelled = mapper
            .map(&notification(
                CodexNotificationMethod::TurnCompleted,
                json!({"threadId":"thr_1","turn":{"id":"turn_1","status":"interrupted"}}),
            ))
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.kind, NativeEventKind::TurnCancelled);

        let failed = mapper
            .map(&notification(
                CodexNotificationMethod::TurnCompleted,
                json!({"threadId":"thr_1","turn":{"id":"turn_2","status":"failed","error":{"message":"provider failed"}}}),
            ))
            .unwrap()
            .unwrap();
        assert_eq!(failed.kind, NativeEventKind::TurnFailed);
    }

    #[test]
    fn approvals_allow_deny_cancel_and_workspace_errors_are_exact() {
        let request = CodexServerRequest {
            id: json!(7),
            method: CodexServerRequestMethod::CommandApproval,
            params: json!({"itemId":"item_1","cwd":"/workspace/project"}),
        };
        let projected = project_approval(
            &request,
            Path::new("/workspace/project"),
            &[PathBuf::from("/workspace")],
        )
        .unwrap();
        assert_eq!(projected.kind, AlfredToolKind::Shell);
        let allow = String::from_utf8(
            encode_approval_decision(&request, AlfredApprovalDecision::Allow, false).unwrap(),
        )
        .unwrap();
        let deny = String::from_utf8(
            encode_approval_decision(&request, AlfredApprovalDecision::Deny, false).unwrap(),
        )
        .unwrap();
        let cancel = String::from_utf8(
            encode_approval_decision(&request, AlfredApprovalDecision::Allow, true).unwrap(),
        )
        .unwrap();
        assert!(allow.contains("accept"));
        assert!(deny.contains("decline"));
        assert!(cancel.contains("cancel"));

        let outside = CodexServerRequest {
            params: json!({"itemId":"item_2","cwd":"/private"}),
            ..request
        };
        assert_eq!(
            project_approval(
                &outside,
                Path::new("/workspace/project"),
                &[PathBuf::from("/workspace")],
            )
            .unwrap_err()
            .code,
            NativeErrorCode::WorkspaceDenied
        );
    }

    #[test]
    fn mapped_provider_text_is_redacted_by_the_shared_normalizer() {
        let mut mapper = CodexEventMapper::default();
        let event = mapper
            .map(&notification(
                CodexNotificationMethod::AgentMessageDelta,
                json!({"delta":"Authorization: Bearer provider-secret"}),
            ))
            .unwrap()
            .unwrap();
        let normalized = NativeEventNormalizer::new(NativeEventLimits::default())
            .unwrap()
            .normalize(event)
            .unwrap();
        assert!(!normalized.text.unwrap().contains("provider-secret"));
    }
}
