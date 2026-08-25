use super::process::{cwd_from_request, find_bin, prefer_stdout, run_cmd};
use super::{
    map_cmd_err, AgentActivity, AgentActivityKind, AgentActivityState, AgentAdapter, AgentError,
    AgentProvider, AgentRequest, AgentResponse, AgentRunHooks,
};
use serde_json::Value;
use std::time::Duration;

/// Model id meaning "whatever the CLI is already configured to use".
///
/// pi and OMP route to 15+ providers, so no hardcoded model id is valid for
/// every install. This sentinel omits `--model` and lets the CLI pick.
pub(super) const CLI_DEFAULT_MODEL: &str = "default";

/// pi (<https://pi.dev>) and OMP (<https://omp.sh>) are the same harness: one
/// CLI surface, one JSON event stream. Both adapters drive this runner with
/// their own binary and unattended flags.
pub(super) struct PiFamilyCli {
    pub provider: AgentProvider,
    pub bin: &'static str,
    pub missing_hint: &'static str,
    /// Flags the CLI needs to run without a human at the keyboard.
    pub unattended_args: &'static [&'static str],
}

/// Adapter for the pi coding agent (`pi`).
pub struct PiAdapter;

const PI: PiFamilyCli = PiFamilyCli {
    provider: AgentProvider::Pi,
    bin: "pi",
    missing_hint:
        "pi CLI not found. Install `@earendil-works/pi-coding-agent` and ensure `pi` is on PATH.",
    // pi has no tool-approval prompts, so print mode already runs unattended.
    unattended_args: &[],
};

impl AgentAdapter for PiAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Pi
    }

    fn run(
        &self,
        request: AgentRequest,
        hooks: AgentRunHooks<'_>,
    ) -> Result<AgentResponse, AgentError> {
        run_pi_family(&PI, request, hooks)
    }
}

pub(super) fn run_pi_family(
    cli: &PiFamilyCli,
    request: AgentRequest,
    hooks: AgentRunHooks<'_>,
) -> Result<AgentResponse, AgentError> {
    let name = cli.bin;
    let bin = find_bin(name).ok_or_else(|| AgentError::Message(cli.missing_hint.into()))?;

    let prompt = request.effective_prompt();
    let model = request.effective_model(cli.provider);
    let cwd = cwd_from_request(&request.working_directory);

    let mut args = vec!["-p".to_string(), prompt, "--mode".into(), "json".into()];
    if model != CLI_DEFAULT_MODEL {
        args.push("--model".into());
        args.push(model.clone());
    }
    args.extend(cli.unattended_args.iter().map(|flag| flag.to_string()));

    let on_line = |line: &str| {
        for activity in activities_from_event(name, line) {
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
            "{name} returned empty output{}",
            if output.stderr.trim().is_empty() {
                String::new()
            } else {
                format!(" (stderr: {})", output.stderr.trim())
            }
        )));
    }
    if !output.success {
        // A failed run usually carries its reason on stderr, not in the stream.
        let detail = if output.stderr.trim().is_empty() {
            raw.clone()
        } else {
            output.stderr.trim().to_string()
        };
        return Err(AgentError::Message(format!("{name} failed:\n{detail}")));
    }

    let stream = parse_stream_output(&output.stdout);
    let Some(text) = stream.text.filter(|value| !value.trim().is_empty()) else {
        return Err(AgentError::Message(stream.error.unwrap_or_else(|| {
            format!("{name} completed without an assistant response")
        })));
    };

    let mut metadata = serde_json::json!({
        "provider": cli.provider.as_str(),
        "model": model,
        "bin": bin.display().to_string(),
        "durationMs": output.duration_ms,
    });
    if let (Some(usage), Value::Object(map)) = (stream.usage, &mut metadata) {
        map.insert("usage".into(), usage);
    }

    Ok(AgentResponse {
        output: text,
        metadata,
    })
}

/// Text blocks of one assistant message, joined. Thinking and tool calls are
/// deliberately dropped: the workflow step passes on the answer, not the work.
fn assistant_text(message: &Value) -> String {
    let Some(content) = message.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn is_assistant(message: &Value) -> bool {
    message.get("role").and_then(Value::as_str) == Some("assistant")
}

fn message_error(message: &Value) -> Option<String> {
    message
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn activities_from_event(prefix: &str, line: &str) -> Vec<AgentActivity> {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };

    match event.get("type").and_then(Value::as_str) {
        Some("session") => vec![AgentActivity::new(
            format!("{prefix}:session"),
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            format!("{prefix} session started"),
            event.get("id").and_then(Value::as_str),
        )],
        Some("message_end") => {
            let Some(message) = event.get("message").filter(|m| is_assistant(m)) else {
                return Vec::new();
            };
            if let Some(error) = message_error(message) {
                return vec![AgentActivity::new(
                    format!("{prefix}:error"),
                    AgentActivityKind::Error,
                    AgentActivityState::Completed,
                    format!("{prefix} error"),
                    Some(&error),
                )];
            }
            let text = assistant_text(message);
            if text.trim().is_empty() {
                return Vec::new();
            }
            vec![AgentActivity::new(
                format!("{prefix}:assistant"),
                AgentActivityKind::Assistant,
                AgentActivityState::Completed,
                "Agent response",
                Some(&text),
            )]
        }
        Some("tool_execution_start") => vec![AgentActivity::new(
            tool_activity_id(prefix, &event),
            AgentActivityKind::Tool,
            AgentActivityState::Started,
            event
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("Using tool"),
            None,
        )],
        Some("tool_execution_end") => {
            let failed = event
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            vec![AgentActivity::new(
                tool_activity_id(prefix, &event),
                if failed {
                    AgentActivityKind::Error
                } else {
                    AgentActivityKind::Tool
                },
                AgentActivityState::Completed,
                if failed {
                    "Tool failed"
                } else {
                    "Tool completed"
                },
                event.get("toolName").and_then(Value::as_str),
            )]
        }
        Some("agent_end") => vec![AgentActivity::new(
            format!("{prefix}:turn"),
            AgentActivityKind::Status,
            AgentActivityState::Completed,
            "Work completed",
            None,
        )],
        _ => Vec::new(),
    }
}

fn tool_activity_id(prefix: &str, event: &Value) -> String {
    let id = event
        .get("toolCallId")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    format!("{prefix}:tool:{id}")
}

#[derive(Default)]
struct StreamOutput {
    text: Option<String>,
    usage: Option<Value>,
    error: Option<String>,
}

/// Last assistant message wins: earlier ones narrate tool use, the final one
/// is the answer.
fn parse_stream_output(raw: &str) -> StreamOutput {
    let mut parsed = StreamOutput::default();

    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) != Some("message_end") {
            continue;
        }
        let Some(message) = event.get("message").filter(|m| is_assistant(m)) else {
            continue;
        };
        if let Some(error) = message_error(message) {
            parsed.error = Some(error);
        }
        if let Some(usage) = message.get("usage").filter(|u| u.is_object()) {
            parsed.usage = Some(usage.clone());
        }
        let text = assistant_text(message);
        if !text.trim().is_empty() {
            parsed.text = Some(text);
        }
    }

    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = concat!(
        r#"{"type":"session","version":3,"id":"s1","cwd":"/repo"}"#,
        "\n",
        r#"{"type":"agent_start"}"#,
        "\n",
        r#"{"type":"tool_execution_start","toolCallId":"t1","toolName":"read","args":{}}"#,
        "\n",
        r#"{"type":"tool_execution_end","toolCallId":"t1","toolName":"read","result":{},"isError":false}"#,
        "\n",
        r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Reading"},{"type":"toolCall","id":"t1","name":"read","arguments":{}}],"usage":{"input":10,"output":2},"stopReason":"toolUse"}}"#,
        "\n",
        r#"{"type":"message_update","usage":{},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Do"}}"#,
        "\n",
        r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hidden"},{"type":"text","text":"Done"}],"usage":{"input":20,"output":4},"stopReason":"stop"}}"#,
        "\n",
        r#"{"type":"agent_end","messages":[]}"#,
    );

    #[test]
    fn keeps_the_final_assistant_text_and_drops_thinking() {
        let parsed = parse_stream_output(STREAM);
        assert_eq!(parsed.text.as_deref(), Some("Done"));
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.usage.unwrap()["output"], 4);
    }

    #[test]
    fn streams_session_tool_and_response_activities() {
        let activities = STREAM
            .lines()
            .flat_map(|line| activities_from_event("pi", line))
            .collect::<Vec<_>>();
        let labels = activities
            .iter()
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                "pi session started",
                "read",
                "Tool completed",
                "Agent response",
                "Agent response",
                "Work completed",
            ]
        );
        assert_eq!(activities[3].detail.as_deref(), Some("Reading"));
        assert_eq!(activities[4].detail.as_deref(), Some("Done"));
    }

    #[test]
    fn surfaces_an_assistant_error_message() {
        let raw = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"Rate limited"}}"#;
        let parsed = parse_stream_output(raw);
        assert_eq!(parsed.text, None);
        assert_eq!(parsed.error.as_deref(), Some("Rate limited"));

        let activities = activities_from_event("omp", raw);
        assert_eq!(activities[0].kind, AgentActivityKind::Error);
        assert_eq!(activities[0].detail.as_deref(), Some("Rate limited"));
    }

    #[test]
    fn ignores_non_json_and_user_messages() {
        let raw = ["not json", r#"{"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#].join("\n");
        assert_eq!(parse_stream_output(&raw).text, None);
        assert!(raw
            .lines()
            .flat_map(|line| activities_from_event("pi", line))
            .next()
            .is_none());
    }
}
