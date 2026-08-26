use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    map_cmd_err, AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError,
    AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::Value;
use std::time::Duration;

/// Adapter for Google's Gemini CLI (`gemini`).
///
/// Gemini's stream-json protocol gives Alfred tool and assistant activity as
/// well as a final result. `--skip-trust` is required for scheduled/headless
/// runs in workspaces that have not been approved interactively.
pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Gemini
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        let bin = find_bin("gemini").ok_or_else(|| {
            AgentError::Message(
                "Gemini CLI not found. Install `@google/gemini-cli` and ensure `gemini` is on PATH."
                    .into(),
            )
        })?;

        let prompt = request.effective_prompt();
        let model = request.effective_model(self.provider());
        let cwd = cwd_from_request(&request.working_directory);
        let args = vec![
            "-p".into(),
            prompt,
            "--model".into(),
            model.clone(),
            "--output-format".into(),
            "stream-json".into(),
            "--approval-mode".into(),
            "yolo".into(),
            "--skip-trust".into(),
        ];

        let on_line = |line: &str| {
            for activity in activities_from_event(line) {
                if let Some(callback) = hooks.on_activity {
                    callback(&activity);
                }
            }
        };
        let output = run_cmd(
            &bin,
            &args,
            cwd.as_deref(),
            Duration::from_secs(60 * 15),
            hooks.control,
            Some(&on_line),
        )
        .map_err(map_cmd_err)?;

        let raw = prefer_stdout(&output);
        if raw.trim().is_empty() {
            return Err(AgentError::Message(format!(
                "gemini returned empty output{}",
                if output.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" (stderr: {})", output.stderr.trim())
                }
            )));
        }
        if !output.success {
            return Err(AgentError::Message(format!("gemini failed:\n{raw}")));
        }

        let (text, stats, error) = parse_stream_output(&output.stdout);
        let Some(text) = text.filter(|value| !value.trim().is_empty()) else {
            return Err(AgentError::Message(error.unwrap_or_else(|| {
                "gemini completed without an assistant response".into()
            })));
        };

        let mut metadata = serde_json::json!({
            "provider": "gemini",
            "model": model,
            "bin": bin.display().to_string(),
            "durationMs": output.duration_ms,
        });
        if let Some(Value::Object(extra)) = stats {
            if let Value::Object(map) = &mut metadata {
                map.insert("stats".into(), Value::Object(extra));
            }
        }

        Ok(AgentResponse {
            output: text,
            metadata,
        })
    }
}

fn activities_from_event(line: &str) -> Vec<AgentActivity> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };

    match event.get("type").and_then(Value::as_str) {
        Some("init") => vec![AgentActivity::new(
            "gemini:session",
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Gemini session started",
            event.get("model").and_then(Value::as_str),
        )],
        Some("message") if event.get("role").and_then(Value::as_str) == Some("assistant") => {
            event_text(&event)
                .map(|text| {
                    vec![AgentActivity::new(
                        "gemini:assistant",
                        AgentActivityKind::Assistant,
                        AgentActivityState::Completed,
                        "Agent response",
                        Some(&text),
                    )]
                })
                .unwrap_or_default()
        }
        Some("tool_use") => vec![AgentActivity::new(
            event
                .get("tool_id")
                .or_else(|| event.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("gemini:tool"),
            AgentActivityKind::Tool,
            AgentActivityState::Started,
            event
                .get("tool_name")
                .or_else(|| event.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("Using tool"),
            None,
        )],
        Some("tool_result") => vec![AgentActivity::new(
            event
                .get("tool_id")
                .and_then(Value::as_str)
                .unwrap_or("gemini:tool"),
            AgentActivityKind::Tool,
            AgentActivityState::Completed,
            "Tool completed",
            event.get("status").and_then(Value::as_str),
        )],
        Some("error") => vec![AgentActivity::new(
            "gemini:error",
            AgentActivityKind::Error,
            AgentActivityState::Completed,
            "Gemini error",
            event
                .get("message")
                .or_else(|| event.get("error"))
                .and_then(Value::as_str),
        )],
        Some("result") if event.get("status").and_then(Value::as_str) == Some("error") => {
            vec![AgentActivity::new(
                "gemini:error",
                AgentActivityKind::Error,
                AgentActivityState::Completed,
                "Gemini error",
                event.get("error").and_then(Value::as_str),
            )]
        }
        _ => Vec::new(),
    }
}

fn event_text(event: &Value) -> Option<String> {
    if let Some(text) = event.get("content").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = event.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    let parts = event
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn parse_stream_output(raw: &str) -> (Option<String>, Option<Value>, Option<String>) {
    let mut assistant = Vec::new();
    let mut stats = None;
    let mut error = None;

    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("message") if event.get("role").and_then(Value::as_str) == Some("assistant") => {
                if let Some(text) = event_text(&event) {
                    assistant.push(text);
                }
            }
            Some("result") => {
                stats = event.get("stats").cloned();
                if event.get("status").and_then(Value::as_str) == Some("error") {
                    error = event
                        .get("error")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            Some("error") => {
                error = event
                    .get("message")
                    .or_else(|| event.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    (
        (!assistant.is_empty()).then(|| assistant.join("")),
        stats,
        error,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Activity ids are hashed at the boundary, so a provider id never
    /// reaches a run event verbatim.
    fn is_opaque(id: &str) -> bool {
        id.strip_prefix("agent_activity_").is_some_and(|suffix| {
            suffix.len() == 24 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
        })
    }

    #[test]
    fn parses_gemini_stream_events() {
        let raw = [
            r#"{"type":"init","session_id":"s1","model":"gemini-3-pro-preview"}"#,
            r#"{"type":"tool_use","tool_name":"read_file","tool_id":"tool-1"}"#,
            r#"{"type":"tool_result","tool_id":"tool-1","status":"success"}"#,
            r#"{"type":"message","role":"assistant","content":"Done","delta":true}"#,
            r#"{"type":"result","status":"success","stats":{"output_tokens":4}}"#,
        ]
        .join("\n");

        let (text, stats, error) = parse_stream_output(&raw);
        assert_eq!(text.as_deref(), Some("Done"));
        assert_eq!(
            stats.and_then(|value| value.get("output_tokens").cloned()),
            Some(4.into())
        );
        assert_eq!(error, None);

        let activities = raw
            .lines()
            .flat_map(activities_from_event)
            .collect::<Vec<_>>();
        assert_eq!(activities[0].kind, AgentActivityKind::Status);
        assert_eq!(activities[0].state, AgentActivityState::Completed);
        assert_eq!(activities[0].label, "Gemini session started");

        assert_eq!(activities[1].kind, AgentActivityKind::Tool);
        assert_eq!(activities[1].state, AgentActivityState::Started);
        assert_eq!(activities[1].label, "Using tool");

        assert_eq!(activities[2].kind, AgentActivityKind::Tool);
        assert_eq!(activities[2].state, AgentActivityState::Completed);
        assert_eq!(activities[2].label, "Tool completed");

        assert_eq!(activities[3].kind, AgentActivityKind::Assistant);
        assert_eq!(activities[3].label, "Agent response");

        // The tool call still correlates start to completion opaquely.
        assert_eq!(activities[1].id, activities[2].id);
        assert!(activities.iter().all(|activity| is_opaque(&activity.id)));
        for raw_id in ["tool-1", "s1"] {
            assert!(
                activities.iter().all(|activity| activity.id != raw_id),
                "provider id {raw_id} reached a run event"
            );
        }

        assert!(activities.iter().all(|activity| activity.detail.is_none()));
        let rendered = format!("{activities:?}");
        for leaked in ["Done", "tool-1", "read_file", "gemini-3-pro-preview"] {
            assert!(!rendered.contains(leaked), "leaked {leaked} in {rendered}");
        }
    }
}
