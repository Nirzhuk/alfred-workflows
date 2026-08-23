use super::process::{
    cwd_from_request, find_bin, lock_claude_invocation, prefer_stdout, run_cmd, CmdOutput,
};
use super::{
    map_cmd_err, AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError,
    AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::Value;
use std::time::Duration;

/// Adapter for the Claude Code CLI (`claude`).
///
/// Stream format reference: https://docs.anthropic.com/en/docs/claude-code/cli-usage
pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::ClaudeCode
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("claude").ok_or_else(|| {
            AgentError::Message(
                "claude CLI not found. Install Claude Code and ensure `claude` is on PATH.".into(),
            )
        })?;
        let _invocation = lock_claude_invocation(hooks.control).map_err(map_cmd_err)?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);
        let stream_args = vec![
            "-p".into(),
            prompt.clone(),
            "--model".into(),
            model.clone(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--no-session-persistence".into(),
            "--permission-mode".into(),
            "bypassPermissions".into(),
            "--max-turns".into(),
            "40".into(),
        ];

        let stream_event = |line: &str| {
            for activity in activities_from_event(line) {
                if let Some(callback) = hooks.on_activity {
                    callback(&activity);
                }
            }
        };
        let mut output = run_cmd(
            &bin,
            &stream_args,
            cwd.as_deref(),
            Duration::from_secs(60 * 15),
            hooks.control,
            Some(&stream_event),
        )
        .map_err(map_cmd_err)?;
        let mut streamed = true;

        if !output.success && stream_json_is_unsupported(&prefer_stdout(&output)) {
            streamed = false;
            if let Some(callback) = hooks.on_activity {
                callback(&AgentActivity::new(
                    "claude:waiting",
                    AgentActivityKind::Status,
                    AgentActivityState::Started,
                    "Waiting for final response",
                    None,
                ));
            }
            let fallback_args = vec![
                "-p".into(),
                prompt,
                "--model".into(),
                model.clone(),
                "--output-format".into(),
                "json".into(),
                "--no-session-persistence".into(),
                "--permission-mode".into(),
                "bypassPermissions".into(),
                "--max-turns".into(),
                "40".into(),
            ];
            output = run_cmd(
                &bin,
                &fallback_args,
                cwd.as_deref(),
                Duration::from_secs(60 * 15),
                hooks.control,
                None,
            )
            .map_err(map_cmd_err)?;
        }

        let raw = prefer_stdout(&output);
        if raw.is_empty() {
            return Err(AgentError::Message(format!(
                "claude returned empty output{}",
                if output.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" (stderr: {})", output.stderr.trim())
                }
            )));
        }

        if !output.success {
            return Err(AgentError::Message(
                failure_summary(&raw)
                    .unwrap_or_else(|| format!("claude exited with an error:\n{raw}")),
            ));
        }

        let (text, parsed) = if streamed {
            parse_stream_output(&output.stdout).ok_or_else(|| {
                AgentError::Message("claude completed without an assistant response".into())
            })?
        } else {
            let parsed: Option<Value> = serde_json::from_str(&raw).ok();
            let text = parsed
                .as_ref()
                .and_then(|value| value.get("result"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or(raw);
            (text, parsed)
        };

        Ok(AgentResponse {
            output: text,
            metadata: response_metadata(&model, &bin, &output, parsed.as_ref()),
        })
    }
}

fn stream_json_is_unsupported(raw: &str) -> bool {
    let message = raw.to_ascii_lowercase();
    (message.contains("stream-json") || message.contains("output-format"))
        && (message.contains("unknown")
            || message.contains("unrecognized")
            || message.contains("invalid"))
}

/// Extract a short, human-readable message from a failed stream-json run so
/// users see "You've hit your session limit · resets 10:30am" instead of the
/// whole raw event dump. Returns None when nothing recognizable is present.
fn failure_summary(raw: &str) -> Option<String> {
    let mut result_text = None;
    let mut rate_limit_type = None;
    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("result") => {
                if let Some(text) = event.get("result").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        result_text = Some(text.to_string());
                    }
                }
            }
            Some("rate_limit_event") => {
                rate_limit_type = event
                    .get("rate_limit_info")
                    .and_then(|info| info.get("rateLimitType"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    }
    result_text.or_else(|| {
        rate_limit_type
            .map(|kind| format!("Claude hit its {kind} usage limit. Try again after it resets."))
    })
}

fn activities_from_event(line: &str) -> Vec<AgentActivity> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match event.get("type").and_then(Value::as_str) {
        Some("system") if event.get("subtype").and_then(Value::as_str) == Some("init") => {
            vec![AgentActivity::new(
                "claude:session",
                AgentActivityKind::Status,
                AgentActivityState::Completed,
                "Claude Code session started",
                None,
            )]
        }
        Some("assistant") => assistant_activities(&event),
        Some("user") => tool_result_activities(&event),
        Some("result") if event.get("is_error").and_then(Value::as_bool) == Some(true) => {
            vec![AgentActivity::new(
                "claude:error",
                AgentActivityKind::Error,
                AgentActivityState::Completed,
                "Claude Code error",
                event.get("result").and_then(Value::as_str),
            )]
        }
        _ => Vec::new(),
    }
}

fn assistant_activities(event: &Value) -> Vec<AgentActivity> {
    let message = event.get("message").unwrap_or(event);
    let message_id = message
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("message");
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };

    content
        .iter()
        .enumerate()
        .filter_map(
            |(index, block)| match block.get("type").and_then(Value::as_str) {
                Some("thinking") | Some("reasoning") => Some(AgentActivity::new(
                    format!("claude:thinking:{message_id}:{index}"),
                    AgentActivityKind::Status,
                    AgentActivityState::Started,
                    "Thinking",
                    None,
                )),
                Some("text") => block.get("text").and_then(Value::as_str).map(|text| {
                    AgentActivity::new(
                        format!("claude:assistant:{message_id}:{index}"),
                        AgentActivityKind::Assistant,
                        AgentActivityState::Completed,
                        "Agent response",
                        Some(text),
                    )
                }),
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("claude:tool:{message_id}:{index}"));
                    Some(AgentActivity::new(
                        id,
                        AgentActivityKind::Tool,
                        AgentActivityState::Started,
                        block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Using tool"),
                        None,
                    ))
                }
                _ => None,
            },
        )
        .collect()
}

fn tool_result_activities(event: &Value) -> Vec<AgentActivity> {
    let message = event.get("message").unwrap_or(event);
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        .filter_map(|block| {
            let id = block.get("tool_use_id").and_then(Value::as_str)?;
            let detail = content_text(block.get("content"));
            Some(AgentActivity::new(
                id,
                AgentActivityKind::Tool,
                AgentActivityState::Completed,
                if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                    "Tool failed"
                } else {
                    "Tool completed"
                },
                detail.as_deref(),
            ))
        })
        .collect()
}

fn content_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn parse_stream_output(raw: &str) -> Option<(String, Option<Value>)> {
    let mut assistant_text = Vec::new();
    let mut result = None;
    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let message = event.get("message").unwrap_or(&event);
                if let Some(content) = message.get("content").and_then(Value::as_array) {
                    assistant_text.extend(
                        content
                            .iter()
                            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .map(str::to_string),
                    );
                }
            }
            Some("result") => result = Some(event),
            _ => {}
        }
    }
    let terminal = result
        .as_ref()
        .and_then(|value| value.get("result"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    terminal
        .or_else(|| (!assistant_text.is_empty()).then(|| assistant_text.join("\n")))
        .map(|text| (text, result))
}

fn response_metadata(
    model: &str,
    bin: &std::path::Path,
    output: &CmdOutput,
    parsed: Option<&Value>,
) -> Value {
    let mut metadata = serde_json::json!({
        "provider": "claude_code",
        "model": model,
        "bin": bin.display().to_string(),
        "durationMs": output.duration_ms,
    });
    if let Some(value) = parsed {
        let usage = value.get("usage");
        metadata["durationMs"] = value
            .get("duration_ms")
            .cloned()
            .unwrap_or(metadata["durationMs"].clone());
        metadata["numTurns"] = value.get("num_turns").cloned().unwrap_or(Value::Null);
        metadata["totalCostUsd"] = value.get("total_cost_usd").cloned().unwrap_or(Value::Null);
        metadata["inputTokens"] = usage
            .and_then(|item| item.get("input_tokens"))
            .cloned()
            .unwrap_or(Value::Null);
        metadata["outputTokens"] = usage
            .and_then(|item| item.get("output_tokens"))
            .cloned()
            .unwrap_or(Value::Null);
        metadata["cacheReadTokens"] = usage
            .and_then(|item| item.get("cache_read_input_tokens"))
            .cloned()
            .unwrap_or(Value::Null);
        metadata["cacheCreationTokens"] = usage
            .and_then(|item| item.get("cache_creation_input_tokens"))
            .cloned()
            .unwrap_or(Value::Null);
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_stream_events_without_exposing_thinking_or_tool_input() {
        let events = [
            r#"{"type":"system","subtype":"init","session_id":"session"}"#,
            r#"{"type":"assistant","message":{"id":"msg_1","content":[{"type":"thinking","thinking":"private reasoning"},{"type":"tool_use","id":"tool_1","name":"Read","input":{"file_path":"secret"}},{"type":"text","text":"Working on it"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool_1","content":"file contents"}]}}"#,
        ];
        let activities = events
            .iter()
            .flat_map(|event| activities_from_event(event))
            .collect::<Vec<_>>();

        assert_eq!(activities[0].label, "Claude Code session started");
        assert_eq!(activities[1].label, "Thinking");
        assert_eq!(activities[1].detail, None);
        assert_eq!(activities[2].id, "tool_1");
        assert_eq!(activities[2].state, AgentActivityState::Started);
        assert_eq!(activities[3].detail.as_deref(), Some("Working on it"));
        assert_eq!(activities[4].id, "tool_1");
        assert_eq!(activities[4].state, AgentActivityState::Completed);
        assert!(!format!("{activities:?}").contains("private reasoning"));
        assert!(!format!("{activities:?}").contains("file_path"));
    }

    #[test]
    fn parses_final_stream_result_and_metadata_record() {
        let raw = [
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Draft"}]}}"#,
            r#"{"type":"unknown","future":true}"#,
            r#"{"type":"result","result":"Final","duration_ms":42,"num_turns":2,"usage":{"input_tokens":3,"output_tokens":5}}"#,
        ]
        .join("\n");
        let (text, result) = parse_stream_output(&raw).expect("result");
        assert_eq!(text, "Final");
        assert_eq!(result.unwrap()["num_turns"], 2);
    }

    #[test]
    fn recognizes_only_stream_format_compatibility_errors() {
        assert!(stream_json_is_unsupported(
            "unknown value stream-json for --output-format"
        ));
        assert!(!stream_json_is_unsupported("authentication failed"));
    }

    #[test]
    fn summarizes_rate_limit_failure_instead_of_dumping_raw_stream() {
        let raw = [
            r#"{"type":"system","subtype":"hook_started","hook_name":"SessionStart:startup"}"#,
            r#"{"type":"assistant","message":{"model":"<synthetic>","content":[{"type":"text","text":"You've hit your session limit · resets 10:30am (Europe/Madrid)"}]}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","overageStatus":"rejected"}}"#,
            r#"{"is_error":true,"terminal_reason":"api_error","api_error_status":429,"result":"You've hit your session limit · resets 10:30am (Europe/Madrid)","type":"result"}"#,
        ]
        .join("\n");
        assert_eq!(
            failure_summary(&raw).as_deref(),
            Some("You've hit your session limit · resets 10:30am (Europe/Madrid)")
        );
    }

    #[test]
    fn describes_rate_limit_when_no_result_line_is_present() {
        let raw = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour"}}"#;
        assert_eq!(
            failure_summary(raw).as_deref(),
            Some("Claude hit its five_hour usage limit. Try again after it resets.")
        );
    }

    #[test]
    fn returns_none_for_unparseable_failure_output() {
        assert_eq!(failure_summary("panic: something blew up"), None);
    }
}
